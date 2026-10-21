import urllib.request
import os

icons = {
    'audio.svg': 'audio-lines',
    'trash.svg': 'trash',
    'subtitles.svg': 'captions',
    'film.svg': 'film',
    'sliders.svg': 'sliders-horizontal',
}

out_dir = r'd:\another_encoder\src\ui\icons'
os.makedirs(out_dir, exist_ok=True)

for filename, lucide_name in icons.items():
    url = f'https://raw.githubusercontent.com/lucide-icons/lucide/main/icons/{lucide_name}.svg'
    try:
        req = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0'})
        with urllib.request.urlopen(req, timeout=10) as resp:
            content = resp.read().decode('utf-8')
            content = content.replace('stroke="currentColor"', 'stroke="white"')
            p = os.path.join(out_dir, filename)
            with open(p, 'w', encoding='utf-8') as out:
                out.write(content)
            print(f'Downloaded {filename} ({lucide_name})')
    except Exception as e:
        print(f'Error downloading {filename}: {e}')

# Ensure solid filled playback icons for maximum clarity and punchiness
with open(os.path.join(out_dir, 'play.svg'), 'w', encoding='utf-8') as f:
    f.write('<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="white"><polygon points="6 4 20 12 6 20 6 4"/></svg>')

with open(os.path.join(out_dir, 'pause.svg'), 'w', encoding='utf-8') as f:
    f.write('<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="white"><rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/></svg>')

with open(os.path.join(out_dir, 'stop.svg'), 'w', encoding='utf-8') as f:
    f.write('<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="white"><rect x="4" y="4" width="16" height="16" rx="2"/></svg>')

print("All icons successfully created and saved!")
