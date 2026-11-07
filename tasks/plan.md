# Implementation Plan - UI & Layout Overhaul

Comprehensive UI and layout overhaul of **Another Encoder** based on the findings from `better-colors`, `better-interface`, `better-layout`, `better-typography`, and `better-ui`.

## Problem Statement
The audit revealed several critical and systemic UX/UI issues:
1. **Accessibility**: Custom buttons and interactive controls lack keyboard navigation and focus rings.
2. **Layout Adaptivity**: The PlayerTab trimming/transport deck overflows and clips at the minimum window width (960px) when side panels are open.
3. **Color Semantics & Contrast**: Pausing playback turns alarming danger-red; `Theme.text_muted` and `text_faint` fail WCAG AA 4.5:1 contrast; 393 ad-hoc hex literals bypass tokens; multiple competing saturated buttons per view.
4. **Typography**: 15 arbitrary font sizes including sub-10px micro-text (7.5px–9.5px); rapidly changing numeric readouts lack tabular/monospace formatting causing horizontal jitter.
5. **Surface Polish**: Nested border radius concentricity mismatches; buttons lack tactile 0.96 scale press states; redundant 1px divider lines clutter layouts.

## Proposed Architecture & Design

### Phase 1: Centralized Design System (`theme.slint`)
- Centralize semantic tokens for surfaces, borders, interactive states, status ramps, and typography scale.
- Update `text_muted` to `#9ca3af` (contrast >= 5.2:1).
- Add semantic font sizes (`font_title: 18px`, `font_section: 14px`, `font_body: 13px`, `font_caption: 12px`, `font_badge: 11px`) and tabular monospace family.
- Define dedicated audio meter and timeline tokens (`meter_bg`, `meter_border`, `meter_green`, `meter_yellow`, `meter_red`).

### Phase 2: Component Architecture & Tactile States (`src/ui/components/`)
- Update `ActionButton` and `IconButton`:
  - Bind to `Theme` tokens (no raw hex literals).
  - Add `0.96` scale active press animation (`150ms ease-out`).
  - Standardize height tiers (Small 28px, Medium 34px, Large 40px).
  - Add visible keyboard focus ring.
- Update `VolumeBar` and `VolumeMeter`:
  - Import and use `Theme` tokens.
  - Replace 8px micro-labels with clean 11px badges / legible marks.
- Update `ToggleSwitch`, `NumInput`, `Badge`, and `WindowControls`:
  - Comply with theme tokens and the 11px typography floor.

### Phase 3: Shell & Responsive Workspace (`main.slint`, `queue_panel.slint`)
- Make `QueuePanel` collapsible (expanding to 300px or collapsing to 36px), reclaiming 264px of horizontal room for editing.
- Fix concentric border radii on queue cards (`outer = inner + padding`).
- Replace micro-text (9.5px) in `QueuePanel` with 11px/12px tokens.
- Add confirmation guard on destructive "Clear Queue".
- Bind status bar telemetry to monospace tabular font.

### Phase 4: Media Player & Trimming Studio (`player_tab.slint`, `timeline/`)
- Fix Pause button color: replace destructive red (`#e11d48`) with accent/neutral active state.
- Redesign the transport & trimming deck to responsively wrap so controls fit smoothly at 960px window width.
- Bind `player_time_text` and `player_duration_text` to monospace tabular font.
- Elevate "Lossless Cut Now" as the primary action; keep secondary actions neutral.
- Replace sub-10px fonts in `audio_waveform.slint` and `timeline_grid.slint`.

### Phase 5: Downloader & Transcoder Views (`downloader_tab.slint`, `transcoder_tab.slint`, `edit_panel.slint`)
- DownloaderTab:
  - Consolidate colors to `Theme`.
  - Tabular monospace font for speed, ETA, and progress.
  - Single primary emphasis on "Start Download".
  - Two-step confirmation on "Clear History".
- TranscoderTab:
  - Remove redundant 1px divider lines; group settings by cards and spacing.
- EditPanel:
  - Replace 9px/9.5px dimension labels with 11px/12px readable typography.
  - Natural title case for all labels.

### Phase 6: Verification & Automated Tests
- Create `tests/ui_layout_overhaul_tests.rs` covering:
  - Theme tokens, contrast, and font scale floors.
  - Responsive layout constraints and narrow viewport calculations.
  - Button scale and focus state logic.
  - Collapsible queue panel width states.
- Run `cargo check` and `cargo test`.
