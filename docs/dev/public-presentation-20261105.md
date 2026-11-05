# Public repository presentation — 2026-11-05

User authorization: make felipetomelin/mercury public and prepare a clear,
visually attractive project presentation using relevant repository references.

## References and choices

- [Crush](https://github.com/charmbracelet/crush): short product introduction,
  visible features and installation instructions.
- [Goose](https://github.com/aaif-goose/goose): concise positioning, a direct
  getting-started section and links to deeper documentation.
- [CodexBar](https://github.com/steipete/CodexBar): clear provider/usage framing,
  visual presentation and explanation of the data the providers expose.

Applied structure: original vector hero, short introduction, clear navigation,
two current interface images, six-feature table, source-build instructions,
opt-in knowledge example, platform status and links to advanced material.
English README plus a Portuguese version; no copied branding or screenshots.
Static badges only describe verified metadata; no invented CI/release status.

## Images and privacy

The optional exporter uses the application's real Rust renderer and fictional
state. It does not connect to a Mercury server, provider account or vault.
Cron dates are calculated by the existing scheduling engine for the depicted
weekday schedule. Python renders frame cells into PNG with the actual colors,
terminal geometry and an explicit fictional-data caption.

Original vector sources and reproduction instructions are in
`assets/mercury/`. The social-preview PNG is prepared as a separate asset;
this does not claim it was uploaded to GitHub's custom social-preview setting.
Old upstream screenshots are not used as current Mercury screenshots.

Local path examples in current setup/spec/activation documents were made generic.
Existing Git history and commit authorship were retained; this change does not
purge historical path examples from previous commits. The pre-publication review
checks current files and reachable Git blobs for recognizable credential/private
key patterns; it is not a claim of exhaustive secret detection.

## Attribution

The base README identified Apache-2.0 but the imported tree lacked LICENSE.
Restored the exact license from
https://raw.githubusercontent.com/herdrdev/herdr/v0.9.3/LICENSE
SHA-256: `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4`.
Added NOTICE with source/version and Mercury's modifications, preserved vendored
notices, and corrected the development instruction about upstream attribution.
Cargo metadata now identifies the repository and Apache-2.0 license.

## Scope and verification

Documentation, metadata, original assets, issue templates and an opt-in fixture
exporter. One production style adjustment improves contrast of Usage help text;
no workflow or data behavior changed. Development details formerly in README
remain in DEVELOPMENT.md.

Verification includes renderer export/visual review, Usage tests, Markdown/HTML
local links, image dimensions and text, YAML parsing, Cargo metadata, formatting,
Clippy, build and diff checks. External scanners remain waived by the existing
project policy; no release binaries, feed or CI badge were invented.

### Final review and results

Consolidated local review: PASS; no blocking findings in the changed documentation,
asset exporter, Cargo metadata or Usage contrast adjustment. No Rust-specific
global rule is installed. The exporter is explicitly ignored during normal test
runs and writes only to the directory supplied for manual asset generation.

- Usage tests: 9 passed, 0 failed.
- Explicit renderer export: passed; regenerated PNGs visually inspected.
- Workspace Clippy with all targets and warnings denied: passed.
- Locked offline binary build, formatting and diff checks: passed.
- Markdown local links, issue-template YAML and four PNG files: validated.
- Current files and 3,014 reachable historical Git blobs: no recognizable
  credential/private-key pattern findings; this is a limited pattern scan.

Public presentation is ready for the authorized commit, push and visibility
change. A separate GitHub social-preview upload is not part of this verification.
