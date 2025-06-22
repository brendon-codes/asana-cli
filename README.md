# asana-cli

`asana-cli` is a Rust CLI for querying and operating against the Asana REST API. The binary command is `asana`.

The `cmd` command is generated from the checked-in Asana OpenAPI snapshot in `references/asana-openapi.json`. The generated registry lives at `src/asana/operations.json` and currently exposes 247 operations.

## Build

```sh
cargo build
cargo test
```

Run the local binary during development:

```sh
cargo run -- --help
cargo run -- cmd --help
```

## Configuration

Project configuration lives at `~/.asana/asana.jsonc`. The file is JSON with comments and uses camelCase keys:

- `asanaAccessToken`
- `asanaWorkspaceGid`
- `mode`, either `dryrun` or `live`
- optional `asanaBaseUrl`, defaulting to `https://app.asana.com/api/1.0`

Create a starter config:

```sh
asana util make-config
```

Generated config defaults to `"mode": "dryrun"`. Keep it in dry-run mode until you intentionally want network calls. The checked-in `examples/.asana/asana.jsonc` file is the template copied into `~/.asana/asana.jsonc`.

Validate the config:

```sh
asana util validate-config
```

Print redacted local status:

```sh
asana util status
```

When `mode` is `live`, `status` calls the configured workspace endpoint. In `dryrun` mode it never performs network I/O. CLI output redacts the access token and tests assert that token values do not leak.

## API Commands

Run an Asana REST operation by OpenAPI operation ID. JSON output is the default:

```sh
asana cmd getWorkspaces
asana cmd getTask --task_gid 123
asana cmd createTask --body '{"data":{"workspace":"1200123456789","name":"Sample task"}}'
```

Operation arguments use the OpenAPI parameter names as double-hyphen flags. Path and query parameters are passed as named flags, JSON request bodies use `--body`, and multipart attachment uploads use `--file` plus form fields:

```sh
asana cmd getProjects --workspace 1200123456789
asana cmd getTasks --workspace 1200123456789 --opt_fields gid,name --limit 10
asana cmd createProject --body '{"data":{"workspace":"1200123456789","name":"Launch plan"}}'
asana cmd createAttachmentForObject --parent 123 --file ./report.pdf
asana cmd createWebhook --body '{"data":{"resource":"123","target":"https://example.com/asana-webhook"}}'
```

Use `--markdown` or `--text` for console-friendly output:

```sh
asana cmd --markdown getTask --task_gid 123
```

When config `mode` is `dryrun`, `asana cmd` validates arguments and prints the request that would be made without performing network I/O. Authorization headers are shown as `Bearer <redacted>`.

## Mock Server

Run a local resettable mock Asana API:

```sh
asana server --host 127.0.0.1 --port 0
```

The server prints the selected base URL, for example `http://127.0.0.1:54321/api/1.0`. Use that URL with live `cmd` calls:

```sh
asana cmd --base-url http://127.0.0.1:54321/api/1.0 getWorkspaces
asana cmd --base-url http://127.0.0.1:54321/api/1.0 createTask --body '{"data":{"workspace":"1200123456789","name":"Mock task"}}'
```

Mock data is stored as JSON under `.asana/data/` unless `--data-dir` is provided. The server resets its managed mock files on startup and graceful shutdown while leaving unrelated files in the data directory alone.

## Project Skills

Copy the project `asana-cli` end-user skill into another repo-local agent skill directory:

```sh
asana util make-skill codex
asana util make-skill claude
```

The command copies the checked-in project skill file to `.codex/skills/asana-cli/` or `.claude/skills/asana-cli/` under the current Git repository root. An existing destination `asana-cli` skill directory is replaced.

## References

- `ARCHITECTURE.md`: module layout, registry, config, request/output model, mock server, and skill generation behavior.
- `references/operation-index.md`: generated operation index from the checked-in registry.
- `references/argument-model.md`: how OpenAPI parameters map to CLI arguments and mock requests.
- `plans/initial/*.md`: staged implementation plans and release-readiness requirements.
