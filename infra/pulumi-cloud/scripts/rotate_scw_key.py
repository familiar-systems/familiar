"""Rotate a Scaleway IAM ApiKey on a given iam.Application.

Encodes the rotation state machine so the user never has to walk through
it manually again. State machine:

    1. enumerate    list current api-keys for <app-id>; require exactly one.
    2. mint         scw iam api-key create application_id=...
    3. apply        gha:   gh secret set SCW_ACCESS_KEY/SCW_SECRET_KEY
                    local: scw config set + ./scripts/setup.sh regen of .envrc
    4. verify       gha:   gh workflow run ci_cd_main.yml + infra.yml on main,
                           gh run watch --exit-status (block until green).
                    local: scw account info + pulumi preview --non-interactive
                           (with os.environ refreshed from the new .envrc).
    5. revoke       scw iam api-key delete <old> (skipped with --keep-old).
    6. report       summary of what changed.

Auth model: reads the current shell's SCW credentials from the env (typically
loaded via direnv from infra/pulumi-cloud/.envrc). That key must have IAM
permission to mint and delete api-keys on the target Application; in
practice this means an admin key. For target=local, the script swaps the
process env mid-run after step 3 so steps 4 and 5 use the NEW credentials.
"""

# ruff: noqa: S603
# Subprocess inputs come from click-validated args + Scaleway/GitHub API
# responses parsed through pydantic models, not arbitrary user input.

from __future__ import annotations

import os
import re
import shutil
import subprocess
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import ClassVar, Final, Literal, get_args

import click
from pydantic import BaseModel, ConfigDict, Field, SecretStr, TypeAdapter

REPO_DEFAULT = "familiar-systems/familiar"
GHA_VERIFY_WORKFLOWS = ("ci_cd_main.yml", "infra.yml")
SCRIPT_DIR = Path(__file__).resolve().parent
PULUMI_DIR = SCRIPT_DIR.parent

Target = Literal["gha", "local"]
TARGETS: Final[tuple[Target, ...]] = get_args(Target)


class Application(BaseModel):
    """Scaleway iam.Application response shape (subset we depend on)."""

    model_config: ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="ignore")

    id: str
    name: str
    description: str | None = None


class ApiKey(BaseModel):
    """Scaleway iam.ApiKey response shape (subset we depend on).

    `application_id` is optional because `scw iam api-key list` returns
    both application-owned and user-owned keys in one stream; user-owned
    keys carry `user_id` instead and have no `application_id` field. The
    filter in `list_keys_for_app` naturally drops user-owned keys because
    `None == app_id` is `False`.
    """

    model_config: ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="ignore")

    access_key: str
    secret_key: SecretStr | None = None
    application_id: str | None = None
    created_at: str | None = None
    description: str | None = None


class WorkflowRun(BaseModel):
    """`gh run list --json databaseId,status,conclusion,url,createdAt` row.

    `gh` emits camelCase JSON; we snake_case at the Python layer via aliases.
    """

    model_config: ClassVar[ConfigDict] = ConfigDict(extra="ignore", populate_by_name=True)

    database_id: int = Field(alias="databaseId")
    status: str
    conclusion: str | None = None
    url: str
    created_at: str = Field(alias="createdAt")


# Pydantic doesn't auto-snake-case incoming JSON keys; gh's output is camelCase.
# Re-declaring the field with an alias would clutter the model; instead we point
# basedpyright + the runtime at the same camelCase keys via populate_by_name +
# model_validate(by_name=False) on each call. Keeping the field names snake_case
# at the Python layer matters because that is the convention the rest of the file
# uses; the JSON-shape adapter below applies the rename once.
_RUN_LIST_ADAPTER = TypeAdapter(list[WorkflowRun])
_API_KEY_LIST_ADAPTER = TypeAdapter(list[ApiKey])
_APP_LIST_ADAPTER = TypeAdapter(list[Application])


def run(
    cmd: list[str],
    *,
    capture: bool = True,
    check: bool = True,
    cwd: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    """Wrapper around subprocess.run that surfaces stderr on captured failures.

    Without this, a CalledProcessError from check=True + capture_output=True
    only carries the command + exit code in its default traceback; the
    captured stderr lives on `.stderr` but Python won't print it. The
    operator then sees "exit 1" with no signal as to why. We re-raise as a
    RuntimeError that includes stderr (or stdout, if stderr was empty), so
    the actual subprocess complaint is the first thing in the traceback.
    """
    try:
        return subprocess.run(
            cmd,
            capture_output=capture,
            text=True,
            check=check,
            cwd=cwd,
        )
    except subprocess.CalledProcessError as e:
        if not capture:
            raise
        # subprocess.CalledProcessError types stderr/stdout as Any because the
        # stubs don't condition on the text=True flag; narrow with isinstance.
        detail = e.stderr or e.stdout or "<no captured output>"  # pyright: ignore[reportAny]
        raise RuntimeError(
            f"`{' '.join(cmd)}` failed (exit {e.returncode}):\n{detail.rstrip()}"
        ) from e


def preflight() -> None:
    for tool in ("scw", "gh", "pulumi", "git"):
        if not shutil.which(tool):
            raise FileNotFoundError(f"{tool} not on PATH. Install it before re-running.")
    auth = run(["gh", "auth", "status"], check=False)
    if auth.returncode != 0:
        raise RuntimeError("gh is not authenticated. Run `gh auth login` first.")


def list_keys_for_app(app_id: str) -> list[ApiKey]:
    """Filter client-side. Avoids brittleness around the scw CLI's list-filter
    syntax across versions; pagination on `scw iam api-key list` is handled
    by the CLI itself.
    """
    r = run(["scw", "iam", "api-key", "list", "-o", "json"])
    if not r.stdout.strip():
        return []
    keys = _API_KEY_LIST_ADAPTER.validate_json(r.stdout)
    return [k for k in keys if k.application_id == app_id]


def resolve_application(name: str) -> Application:
    """Resolve a human-readable iam.Application name to the underlying record.

    Names are unique within an Organization in Scaleway IAM. If a future
    contributor manages to create two with the same name, this errors with
    the candidates listed rather than picking one arbitrarily.
    """
    r = run(["scw", "iam", "application", "list", "-o", "json"])
    if not r.stdout.strip():
        raise RuntimeError(
            "scw iam application list returned no applications. "
            "Verify the active scw profile points at the right Organization."
        )
    apps = _APP_LIST_ADAPTER.validate_json(r.stdout)
    matches = [a for a in apps if a.name == name]
    if not matches:
        listing = "\n".join(f"    {a.name}  (id={a.id})" for a in apps)
        raise ValueError(f"No iam.Application named {name!r}. Available:\n{listing}")
    if len(matches) > 1:
        listing = "\n".join(
            f"    {a.name}  id={a.id}  description={a.description!r}" for a in matches
        )
        raise ValueError(
            f"Multiple iam.Applications named {name!r}; can't disambiguate.\n{listing}"
        )
    return matches[0]


def gh_repo() -> str:
    """`familiar-systems/familiar` parsed from `git remote get-url origin`."""
    try:
        r = run(["git", "remote", "get-url", "origin"])
    except subprocess.CalledProcessError:
        return REPO_DEFAULT
    m = re.search(r"github\.com[:/]([^/]+)/([^/.]+)", r.stdout.strip())
    return f"{m.group(1)}/{m.group(2)}" if m else REPO_DEFAULT


def mint(app_id: str, *, dry_run: bool) -> ApiKey:
    description = f"Rotation {datetime.now(UTC).strftime('%Y-%m-%d')}"
    click.echo(f"-- step 2: mint new key on {app_id}  (description={description!r})")
    if dry_run:
        click.echo("--   [dry-run] scw iam api-key create application_id=... description=...")
        # secret_key=None marks this as a dry-run sentinel; downstream
        # apply_*() functions short-circuit on dry_run before reading it.
        return ApiKey(
            access_key="<dry-run-new-access-key>",
            secret_key=None,
            application_id=app_id,
            created_at=datetime.now(UTC).isoformat(),
            description=description,
        )
    r = run(
        [
            "scw",
            "iam",
            "api-key",
            "create",
            f"application-id={app_id}",
            f"description={description}",
            "-o",
            "json",
        ]
    )
    return ApiKey.model_validate_json(r.stdout)


def apply_gha(new: ApiKey, *, dry_run: bool) -> None:
    repo = gh_repo()
    click.echo(f"-- step 3 (gha): set SCW_ACCESS_KEY / SCW_SECRET_KEY on {repo}")
    if dry_run:
        click.echo(f"--   [dry-run] gh secret set SCW_ACCESS_KEY --repo {repo} --body <new>")
        click.echo(f"--   [dry-run] gh secret set SCW_SECRET_KEY --repo {repo} --body <new>")
        return
    if new.secret_key is None:
        raise RuntimeError(
            "Internal error: minted key is missing secret_key.\n"
            "Inspect Scaleway IAM and revoke the dangling key manually."
        )
    _ = run(
        ["gh", "secret", "set", "SCW_ACCESS_KEY", "--repo", repo, "--body", new.access_key],
        capture=False,
    )
    _ = run(
        [
            "gh",
            "secret",
            "set",
            "SCW_SECRET_KEY",
            "--repo",
            repo,
            "--body",
            new.secret_key.get_secret_value(),
        ],
        capture=False,
    )


def apply_local(new: ApiKey, *, dry_run: bool) -> None:
    click.echo("-- step 3 (local): scw config set + .envrc regen via setup.sh")
    if dry_run:
        click.echo(f"--   [dry-run] scw config set access-key={new.access_key}")
        click.echo("--   [dry-run] scw config set secret-key=<new>")
        click.echo(f"--   [dry-run] cd {PULUMI_DIR} && bash scripts/setup.sh")
        return
    if new.secret_key is None:
        raise RuntimeError(
            "Internal error: minted key is missing secret_key. Revoke the dangling key manually."
        )
    _ = run(["scw", "config", "set", f"access-key={new.access_key}"], capture=False)
    _ = run(
        ["scw", "config", "set", f"secret-key={new.secret_key.get_secret_value()}"],
        capture=False,
    )
    setup = PULUMI_DIR / "scripts" / "setup.sh"
    _ = run(["bash", str(setup)], cwd=PULUMI_DIR, capture=False)


def trigger_and_watch_workflow(workflow: str, repo: str) -> None:
    click.echo(f"--   trigger {workflow}")
    _ = run(["gh", "workflow", "run", workflow, "--ref", "main", "--repo", repo], capture=False)
    # GitHub registers the run a moment after dispatch returns; ~3-5s usually
    # suffices. Retry the lookup to absorb flake.
    run_id: str | None = None
    run_url: str | None = None
    for _attempt in range(10):
        time.sleep(3)
        r = run(
            [
                "gh",
                "run",
                "list",
                "--workflow",
                workflow,
                "--repo",
                repo,
                "--event",
                "workflow_dispatch",
                "--limit",
                "1",
                "--json",
                "databaseId,status,conclusion,url,createdAt",
            ]
        )
        if not r.stdout.strip():
            continue
        runs = _RUN_LIST_ADAPTER.validate_json(r.stdout)
        if runs:
            run_id = str(runs[0].database_id)
            run_url = runs[0].url
            break
    if run_id is None:
        raise TimeoutError(
            f"Couldn't locate a triggered run for {workflow} after 30s.\n"
            f"State: NEW key wired into GHA secrets, OLD key still alive.\n"
            f"Investigate at https://github.com/{repo}/actions; if no run started, "
            "the workflow may need `workflow_dispatch:` added to its `on:` block."
        )
    click.echo(f"--   watching {run_url}")
    watch = run(
        ["gh", "run", "watch", run_id, "--repo", repo, "--exit-status"],
        capture=False,
        check=False,
    )
    if watch.returncode != 0:
        raise RuntimeError(
            f"{workflow} did not go green: {run_url}\n"
            "State: NEW key wired into GHA secrets, OLD key still alive.\n"
            "Decide: fix-forward in CI, or restore the OLD secrets via\n"
            "  gh secret set SCW_ACCESS_KEY --body <old>\n"
            "  gh secret set SCW_SECRET_KEY --body <old>\n"
            "and then `scw iam api-key delete <new-access-key>`."
        )


def verify_gha(*, dry_run: bool) -> None:
    repo = gh_repo()
    click.echo(f"-- step 4 (gha): trigger + watch {GHA_VERIFY_WORKFLOWS} on main")
    if dry_run:
        for w in GHA_VERIFY_WORKFLOWS:
            click.echo(
                f"--   [dry-run] gh workflow run {w} --ref main --repo {repo}; gh run watch ..."
            )
        return
    for w in GHA_VERIFY_WORKFLOWS:
        trigger_and_watch_workflow(w, repo)


def reload_env_from_envrc() -> None:
    """Re-source .envrc by parsing `export FOO=bar` lines and writing to
    os.environ. Required between step 3 and step 4 for target=local because
    the running process inherited the OLD credentials at startup; we have to
    propagate the NEW ones into our own environment before invoking
    `scw account info` and `pulumi preview`.
    """
    envrc = PULUMI_DIR / ".envrc"
    if not envrc.exists():
        raise FileNotFoundError(f"Expected {envrc} to exist after setup.sh; it doesn't.")
    for raw_line in envrc.read_text().splitlines():
        line = raw_line.strip()
        if not line.startswith("export "):
            continue
        kv = line[len("export ") :]
        if "=" not in kv:
            continue
        k, _, v = kv.partition("=")
        os.environ[k.strip()] = v.strip().strip('"').strip("'")


def verify_local(*, dry_run: bool) -> None:
    click.echo("-- step 4 (local): scw account info + pulumi preview")
    if dry_run:
        click.echo("--   [dry-run] os.environ <- new .envrc")
        click.echo("--   [dry-run] scw account info")
        click.echo(f"--   [dry-run] cd {PULUMI_DIR} && pulumi preview --non-interactive")
        return
    reload_env_from_envrc()
    r = run(["scw", "account", "info"], check=False)
    if r.returncode != 0:
        raise RuntimeError(
            f"`scw account info` failed with the new key:\n{r.stderr}\n"
            "State: scw config + .envrc carry the NEW credentials, OLD key still alive.\n"
            "To roll back: edit ~/.config/scw/config.yaml back to the OLD pair, re-run\n"
            "`./scripts/setup.sh`, then `scw iam api-key delete <new-access-key>`."
        )
    preview = run(
        ["pulumi", "preview", "--non-interactive"],
        cwd=PULUMI_DIR,
        capture=False,
        check=False,
    )
    if preview.returncode != 0:
        raise RuntimeError(
            "`pulumi preview` failed with the new key. Same state and rollback as above."
        )


def revoke(old: ApiKey, *, dry_run: bool) -> None:
    click.echo(f"-- step 5: revoke old key {old.access_key}")
    if dry_run:
        click.echo(f"--   [dry-run] scw iam api-key delete {old.access_key}")
        return
    _ = run(["scw", "iam", "api-key", "delete", old.access_key], capture=False)


def report(
    app: Application,
    old: ApiKey,
    new: ApiKey,
    *,
    target: Target,
    revoked: bool,
    dry_run: bool,
) -> None:
    bar = "=" * 72
    suffix = "  (DRY RUN)" if dry_run else ""
    desc_line = (
        f"\n  new description : {new.description}" if (not dry_run and new.description) else ""
    )
    body = (
        f"\n{bar}\n"
        f"Rotation summary [{target}]{suffix}\n"
        f"{bar}\n"
        f"  application     : {app.name}  (id={app.id})\n"
        f"  old access_key  : {old.access_key}  ({'revoked' if revoked else 'KEPT'})\n"
        f"  new access_key  : {new.access_key}{desc_line}\n"
        f"  finished at     : {datetime.now(UTC).isoformat()}\n"
    )
    click.echo(body)


def _coerce_target(_ctx: click.Context, _param: click.Parameter, value: str) -> Target:
    """Click validates membership via Choice; this narrows str -> Target for callees.

    Iterating over TARGETS (which is `tuple[Target, ...]`) gives each element
    the Target type at narrowing-time, so the `return t` branch needs no
    cast or # type: ignore.
    """
    for t in TARGETS:
        if value == t:
            return t
    raise click.BadParameter(f"Unknown target: {value!r}; expected one of {TARGETS}.")


@click.command(context_settings={"help_option_names": ["-h", "--help"]})
@click.argument("application_name", metavar="APPLICATION")
@click.argument(
    "target",
    type=click.Choice(TARGETS),
    callback=_coerce_target,
)
@click.option(
    "--dry-run",
    "dry_run",
    is_flag=True,
    default=False,
    help="Print actions; mint nothing, set nothing, revoke nothing.",
)
@click.option(
    "--keep-old",
    "keep_old",
    is_flag=True,
    default=False,
    help="Skip step 5; the old key remains alive after verification.",
)
def main(application_name: str, target: Target, *, dry_run: bool, keep_old: bool) -> None:
    """Rotate a Scaleway IAM api-key on a given iam.Application.

    APPLICATION is the iam.Application name (lookup: `scw iam application list`).
    TARGET is the consumer of the rotated credential: `gha` or `local`.
    """
    preflight()

    click.echo(f"-- resolve application {application_name!r}")
    app = resolve_application(application_name)
    click.echo(f"--   {app.name}  id={app.id}")

    click.echo(f"-- step 1: enumerate keys on {app.name}")
    keys = list_keys_for_app(app.id)
    if not keys:
        raise ValueError(
            f"application {app.name} (id={app.id}) has 0 api-keys.\n"
            "This is unexpected. If intentional, mint the first key in the Scaleway console."
        )
    if len(keys) > 1:
        listing = "\n".join(
            f"    {k.access_key}  created_at={k.created_at}  description={k.description!r}"
            for k in keys
        )
        raise ValueError(
            f"application {app.name} (id={app.id}) has {len(keys)} api-keys (expected 1).\n"
            "Ambiguous: pick the one to revoke and run `scw iam api-key delete <id>` first, "
            f"then re-run.\n{listing}"
        )
    old = keys[0]
    click.echo(f"--   old key: {old.access_key}  created_at={old.created_at}")

    new = mint(app.id, dry_run=dry_run)
    click.echo(f"--   new key: {new.access_key}")

    match target:
        case "gha":
            apply_gha(new, dry_run=dry_run)
            verify_gha(dry_run=dry_run)
        case "local":
            apply_local(new, dry_run=dry_run)
            verify_local(dry_run=dry_run)

    revoked = False
    if keep_old:
        click.echo("-- step 5: --keep-old set; old key remains alive.")
    else:
        revoke(old, dry_run=dry_run)
        revoked = not dry_run

    report(app, old, new, target=target, revoked=revoked, dry_run=dry_run)


if __name__ == "__main__":
    main()
