# metactl v1 Sanitized Export

Private source material never becomes public by default. A sanitized export is an explicit record containing source artifact, sanitizer transform, dropped fields, reviewer diff path, original digest, sanitized digest, export time, applied sanitizer IDs, review status, and public-boundary result.

The public fixture is `fixtures/v1/sanitized-export.sample.json`; unsafe private markers are still rejected by `make verify-public-boundary`.
