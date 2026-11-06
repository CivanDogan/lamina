# Publishing a release

The [Release workflow](../.github/workflows/release.yml) builds `lamina` and
`lamina-viewer` for Windows, Linux and macOS, on x64 and Arm64. It packages the
programs with the README, licences and SHA-256 checksums.

A manual run builds downloadable artifacts. A pushed version tag builds the
artifacts and publishes a GitHub Release after all six builds pass.

## Before a release

1. Merge the changes to `master`.
2. Set `workspace.package.version` in the root `Cargo.toml` to the release version.
   Run `cargo check --workspace --locked`. If the version change requires a lockfile
   update, run `cargo check --workspace` and include `Cargo.lock` in the commit.
3. Update [.github/release-notes.md](../.github/release-notes.md) with the changes
   and any new install requirements.
4. Check that the [CI run](https://github.com/FarmingOps/lamina/actions/workflows/ci.yml)
   for the release commit passes. It checks formatting, Clippy and workspace tests.

If a job fails before any steps start, read the annotations on its run page.
For an account lock due to billing, the repository owner must resolve the billing
issue before retrying. Changing the workflow or pushing another tag will not fix it.

## Test the archives

Open [Actions → Release](https://github.com/FarmingOps/lamina/actions/workflows/release.yml),
select **Run workflow**, choose `master`, and start the run. With the GitHub CLI:

```sh
gh workflow run release.yml --repo FarmingOps/lamina --ref master
```

A manual run does not publish a release, even when its ref is a tag.
For a branch run, the archive version is `dev-<commit>`.

After the run passes, download the artifacts from the run page. Each artifact is a
wrapper around the platform archive and its checksum file. Extract the artifact,
check the checksum, then extract the platform archive.
Try these commands from the extracted program folder on each available platform:

```sh
./lamina --version
./lamina new smoke/board
./lamina status smoke/board
./lamina-viewer --help
./lamina-viewer smoke/board
```

On Windows PowerShell, use `./lamina.exe` and `./lamina-viewer.exe`.
Check that the viewer opens the board, then close it. The workflow smoke-tests
native builds without opening a window; it builds the Intel macOS archive on an
Apple silicon runner without running its programs. Test that archive on an Intel Mac.

## Publish

Use an account with write access. Start from a clean working tree and the tested
release commit on `master`. For version `0.1.0`:

```sh
git switch master
git pull --ff-only origin master
git tag -a v0.1.0 -m "Lamina 0.1.0"
git push origin v0.1.0
```

Before tagging, confirm that the commit is still the one you tested and that
`workspace.package.version` is `0.1.0`. For later releases, change both version
values in these commands. The program version comes from Cargo, so it must match
the tag.

Watch the Release run. After it passes, check the
[Releases page](https://github.com/FarmingOps/lamina/releases) for six archives and
their six checksum files. Download an archive and check that it contains both
programs, the README and both licence files.

If a tag build fails, fix the cause before publishing. Do not move a tag that
already has a public release. If the tagged source needs a change, use a new
version and tag.
