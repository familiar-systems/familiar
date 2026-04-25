# History secret scan: 2026-04-25

Scan of full git history performed before the private→public flip described in [`GHA_CLEANUP.md`](../../GHA_CLEANUP.md).

## Result: clean

| Tool | Mode | Scope | Findings |
| --- | --- | --- | --- |
| `trufflehog` 3.95.2 | `--only-verified` | `git file://.` (all refs, default) | 0 |
| `trufflehog` 3.95.2 | unverified (full pattern set) | `git file://.` (all refs, default) | 0 |
| `gitleaks` 8.30.1 | default ruleset, redacted output | `--source . --log-opts="--all"` (97 commits, all branches) | 0 |

Scanners installed via `mise.toml`. Run from `/home/mg/git/familiar-systems/familiar` at HEAD `c725b311eaa2e25e24b5cab6c5bc4f5a52fb8e2d`.

## Why both verified and unverified passes

`trufflehog --only-verified` actively probes each candidate against the live service and reports a hit only if the credential still authenticates. False-positive rate is near zero, but rotated keys and credentials for unreachable services slip through. The unverified pass catches anything that *looks* like a credential format we care about (Scaleway `SCW...` keys, AWS `AKIA...`, generic high-entropy strings flagged by trufflehog's detectors), which still matters once the repo is public because attackers harvest historical strings for credential-reuse attacks even when the original service has rotated. Both passes returning 0 means there is nothing to rotate and nothing to scrub from history.

## Why clean is plausible here

The repo is pre-implementation. Configuration that *could* contain secrets routes through:

- Scaleway Secrets Manager (the project's stated single source of truth for all secrets).
- GitHub Actions repo secrets (`SCW_*`, `PULUMI_*`), referenced in workflows via `secrets.NAME` and never committed.
- `mise.toml` `[env]` for non-secret config (e.g. `HANKO_API_URL_DEV`).

No `.env` files, no `kubeconfig` blobs, no inline private keys. The discipline shows up in the scan.

## Reproduce

```bash
mise exec -- trufflehog git file://. --only-verified
mise exec -- trufflehog git file://.
mise exec -- gitleaks detect --source . --log-opts="--all" --redact
```

## Re-run cadence

Re-run before any future private repo cutover, and on a quarterly basis as long as the repo stays public, to detect drift if a future change accidentally commits a value that current rules would catch.
