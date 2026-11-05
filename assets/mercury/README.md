# Public presentation assets

`hero.svg` and `social-preview.svg` are original vector artwork for Mercury.
`usage.png`, `crons.png` and `knowledge.png` are produced from the application's
actual renderer, with fictional demo data. They contain no personal sessions,
tokens, account identifiers or vault contents. The UI currently includes
Portuguese labels. `social-preview.png` is ready for GitHub's social preview UI.

To regenerate the screenshots, from the repository root:

```sh
MERCURY_README_FRAMES=/tmp/mercury-readme-frames \
  cargo test --bin mercury export_public_readme_frames -- --ignored
python3 scripts/render_readme_assets.py --frames /tmp/mercury-readme-frames
```

The optional Python renderer needs Pillow. On Linux, pass
`--font /path/to/monospace.ttf` if DejaVu Sans Mono is unavailable.
PNG text can vary slightly across font installations; the UI frame data is
exported directly from Rust. No agents, models or network services are invoked.
