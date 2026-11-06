use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use super::error::BinaryError;
use super::platform::{ArchiveFormat, PlatformInfo};

pub struct BinaryExtractor;

impl BinaryExtractor {
    /// Safely replaces `dst` with `src`. Attempts an atomic rename first,
    /// falling back to copy-and-delete if crossing filesystem boundaries.
    fn atomic_replace_file(src: &Path, dst: &Path) -> Result<(), BinaryError> {
        if dst.exists() {
            let _ = fs::remove_file(dst);
        }
        if fs::rename(src, dst).is_err() {
            fs::copy(src, dst)?;
            let _ = fs::remove_file(src);
        }
        Ok(())
    }

    /// Extracts or installs downloaded binary assets into `bin_dir`.
    pub fn extract(
        archive_path: &Path,
        bin_dir: &Path,
        format: &ArchiveFormat,
        platform: &PlatformInfo,
    ) -> Result<(), BinaryError> {
        fs::create_dir_all(bin_dir)?;

        match format {
            ArchiveFormat::RawExecutable => {
                let dest_filename = platform.ytdlp_filename();
                let dest_path = bin_dir.join(dest_filename);

                Self::atomic_replace_file(archive_path, &dest_path)?;
                Self::apply_permissions_and_attributes(&dest_path)?;
            }
            ArchiveFormat::Zip => {
                Self::extract_zip(archive_path, bin_dir, platform)?;
                let _ = fs::remove_file(archive_path);
            }
            ArchiveFormat::TarXz => {
                Self::extract_tar_xz(archive_path, bin_dir, platform)?;
                let _ = fs::remove_file(archive_path);
            }
        }

        Ok(())
    }

    /// Extracts matching ffmpeg suite binaries from a .zip archive.
    fn extract_zip(
        archive_path: &Path,
        bin_dir: &Path,
        platform: &PlatformInfo,
    ) -> Result<(), BinaryError> {
        let file = File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let ffmpeg_name = platform.ffmpeg_filename();
        let ffprobe_name = platform.ffprobe_filename();
        let ffplay_name = platform.ffplay_filename();

        let mut extracted_ffmpeg = false;
        let mut extracted_ffprobe = false;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let raw_name = file.name()?.replace('\\', "/");

            let target_name = if raw_name.ends_with(&format!("/{}", ffmpeg_name)) || raw_name == ffmpeg_name {
                extracted_ffmpeg = true;
                Some(ffmpeg_name)
            } else if raw_name.ends_with(&format!("/{}", ffprobe_name)) || raw_name == ffprobe_name {
                extracted_ffprobe = true;
                Some(ffprobe_name)
            } else if raw_name.ends_with(&format!("/{}", ffplay_name)) || raw_name == ffplay_name {
                Some(ffplay_name)
            } else {
                None
            };

            if let Some(out_name) = target_name {
                let out_path = bin_dir.join(out_name);
                let tmp_out_path = bin_dir.join(format!("{}.extract.tmp", out_name));

                let mut outfile = File::create(&tmp_out_path)?;
                io::copy(&mut file, &mut outfile)?;
                outfile.flush()?;
                drop(outfile);

                Self::atomic_replace_file(&tmp_out_path, &out_path)?;
                Self::apply_permissions_and_attributes(&out_path)?;
            }
        }

        if !extracted_ffmpeg {
            return Err(BinaryError::ArchiveExtraction(format!(
                "Archive '{}' did not contain '{}'",
                archive_path.display(),
                ffmpeg_name
            )));
        }

        if !extracted_ffprobe {
            return Err(BinaryError::ArchiveExtraction(format!(
                "Archive '{}' did not contain '{}'",
                archive_path.display(),
                ffprobe_name
            )));
        }

        Ok(())
    }

    /// Extracts matching binaries from a .tar.xz archive.
    fn extract_tar_xz(
        archive_path: &Path,
        bin_dir: &Path,
        platform: &PlatformInfo,
    ) -> Result<(), BinaryError> {
        let file = File::open(archive_path)?;
        let decompressor = xz2::read::XzDecoder::new(file);
        let mut archive = tar::Archive::new(decompressor);

        let ffmpeg_name = platform.ffmpeg_filename();
        let ffprobe_name = platform.ffprobe_filename();
        let ffplay_name = platform.ffplay_filename();

        let mut extracted_ffmpeg = false;
        let mut extracted_ffprobe = false;

        let entries = archive.entries().map_err(|e| {
            BinaryError::ArchiveExtraction(format!("Failed to read tar entries: {}", e))
        })?;

        for entry_res in entries {
            let mut entry = entry_res.map_err(|e| {
                BinaryError::ArchiveExtraction(format!("Failed to read entry: {}", e))
            })?;

            let entry_path = entry.path().map_err(|e| {
                BinaryError::ArchiveExtraction(format!("Invalid entry path: {}", e))
            })?;
            let path_str = entry_path.to_string_lossy().replace('\\', "/");

            let target_name = if path_str.ends_with(&format!("/{}", ffmpeg_name)) || path_str == ffmpeg_name {
                extracted_ffmpeg = true;
                Some(ffmpeg_name)
            } else if path_str.ends_with(&format!("/{}", ffprobe_name)) || path_str == ffprobe_name {
                extracted_ffprobe = true;
                Some(ffprobe_name)
            } else if path_str.ends_with(&format!("/{}", ffplay_name)) || path_str == ffplay_name {
                Some(ffplay_name)
            } else {
                None
            };

            if let Some(out_name) = target_name {
                let out_path = bin_dir.join(out_name);
                let tmp_out_path = bin_dir.join(format!("{}.extract.tmp", out_name));

                let mut outfile = File::create(&tmp_out_path)?;
                io::copy(&mut entry, &mut outfile)?;
                outfile.flush()?;
                drop(outfile);

                Self::atomic_replace_file(&tmp_out_path, &out_path)?;
                Self::apply_permissions_and_attributes(&out_path)?;
            }
        }

        if !extracted_ffmpeg {
            return Err(BinaryError::ArchiveExtraction(format!(
                "Archive '{}' did not contain '{}'",
                archive_path.display(),
                ffmpeg_name
            )));
        }

        if !extracted_ffprobe {
            return Err(BinaryError::ArchiveExtraction(format!(
                "Archive '{}' did not contain '{}'",
                archive_path.display(),
                ffprobe_name
            )));
        }

        Ok(())
    }

    /// Sets POSIX 0o755 execution permissions on Unix and removes macOS quarantine attributes.
    pub fn apply_permissions_and_attributes(path: &Path) -> Result<(), BinaryError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(path, perms)?;
            }
        }

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("xattr")
                .arg("-dr")
                .arg("com.apple.quarantine")
                .arg(path)
                .output();
        }

        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zip::write::{SimpleFileOptions, ZipWriter};

    #[test]
    fn test_extract_zip() {
        let dir = tempdir().unwrap();
        let zip_path = dir.path().join("test.zip");
        let dest_dir = dir.path().join("bin");

        // Create a mock zip archive with ffmpeg and ffprobe inside bin/
        let platform = PlatformInfo::current().unwrap();
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);

        let options = SimpleFileOptions::default();
        zip.start_file(format!("ffmpeg-build/bin/{}", platform.ffmpeg_filename()), options).unwrap();
        zip.write_all(b"mock ffmpeg binary").unwrap();

        zip.start_file(format!("ffmpeg-build/bin/{}", platform.ffprobe_filename()), options).unwrap();
        zip.write_all(b"mock ffprobe binary").unwrap();

        zip.finish().unwrap();

        BinaryExtractor::extract(&zip_path, &dest_dir, &ArchiveFormat::Zip, &platform).unwrap();

        assert!(dest_dir.join(platform.ffmpeg_filename()).is_file());
        assert!(dest_dir.join(platform.ffprobe_filename()).is_file());
    }

    #[test]
    fn test_raw_executable_installation() {
        let dir = tempdir().unwrap();
        let dest_dir = dir.path().join("bin");
        let platform = PlatformInfo::current().unwrap();

        let tmp_file = dir.path().join("yt-dlp.download.tmp");
        fs::write(&tmp_file, b"mock yt-dlp executable").unwrap();

        BinaryExtractor::extract(&tmp_file, &dest_dir, &ArchiveFormat::RawExecutable, &platform).unwrap();

        let installed = dest_dir.join(platform.ytdlp_filename());
        assert!(installed.is_file());
        assert_eq!(fs::read_to_string(installed).unwrap(), "mock yt-dlp executable");
    }
}
