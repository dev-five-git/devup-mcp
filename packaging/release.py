#!/usr/bin/env python3
"""Deliver the workspace version as the canonical, asset-complete MCP release."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.error import HTTPError
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen

SERVER = "crates/devup-mcp/Cargo.toml"
PREFIX = f"devup-mcp({SERVER})@"
SHA = re.compile(r"[0-9a-f]{40}\Z")


class ApiError(RuntimeError):
    def __init__(self, status, path):
        self.status = status
        super().__init__(f"GitHub API returned {status} for {path}")


class GitHub:
    def __init__(self, repository, token):
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
            raise ValueError("Invalid GITHUB_REPOSITORY")
        self.repository, self.token = repository, token

    def request(self, method, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        return self._send(method, f"https://api.github.com/repos/{self.repository}/{path}", data, "application/json")

    def _send(self, method, url, data, content_type):
        request = Request(url, data=data, method=method, headers={
            "Authorization": f"Bearer {self.token}", "Accept": "application/vnd.github+json",
            "Content-Type": content_type, "User-Agent": "devup-mcp-release",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        try:
            with urlopen(request, timeout=120) as response:
                raw = response.read()
                return json.loads(raw) if raw else None
        except HTTPError as error:
            raise ApiError(error.code, urlsplit(url).path) from None

    def upload(self, release, file):
        endpoint = release["upload_url"].split("{", 1)[0]
        expected = f"https://uploads.github.com/repos/{self.repository}/releases/{release['id']}/assets"
        if endpoint != expected:
            raise ValueError("Release upload endpoint does not match its repository and ID")
        return self._send("POST", f"{endpoint}?name={quote(file.name, safe='')}", file.read_bytes(), "application/octet-stream")


def optional(api, path):
    try:
        return api.request("GET", path)
    except ApiError as error:
        if error.status != 404:
            raise
        return None


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True, encoding="utf-8").strip()


def version_at(root, sha=None):
    contents = git(root, "show", f"{sha}:Cargo.toml") if sha else (root / "Cargo.toml").read_text(encoding="utf-8")
    version = tomllib.loads(contents)["workspace"]["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Release delivery requires a stable workspace version")
    return version


def expected_assets(version):
    return {"devup-mcp-linux-x86_64", "devup-mcp-macos-universal", "devup-mcp-windows-x86_64.exe", f"devup-mcp-{version}.mcpb"}


def receipt(release, version, source):
    return {"releaseId": release["id"], "tagName": PREFIX + version, "makeLatest": True, "sourceSha": source, "version": version}


def resolve_tag(api, tag):
    ref = optional(api, "git/ref/tags/" + quote(tag, safe=""))
    if ref is None:
        return None
    obj = ref["object"]
    for _ in range(8):
        if obj["type"] == "commit":
            source = obj["sha"]
            if not SHA.fullmatch(source):
                raise ValueError("Invalid release commit")
            return source
        if obj["type"] != "tag" or not SHA.fullmatch(obj["sha"]):
            break
        obj = api.request("GET", "git/tags/" + obj["sha"])["object"]
    raise ValueError("Release tag does not resolve to a supported commit")


def tag_source(root, api, tag, version, head):
    source = resolve_tag(api, tag)
    if source is None:
        api.request("POST", "git/refs", {"ref": "refs/tags/" + tag, "sha": head})
        return head
    subprocess.run(["git", "merge-base", "--is-ancestor", source, head], cwd=root, check=True)
    if version_at(root, source) != version:
        raise ValueError("Existing release tag has a different workspace version")
    return source


def find_release(api, tag):
    found = []
    for page in range(1, 11):
        releases = api.request("GET", f"releases?per_page=100&page={page}")
        found.extend(release for release in releases if release["tag_name"] == tag)
        if len(releases) < 100:
            break
    else:
        raise ValueError("Release inventory exceeds its bounded page limit")
    if len(found) > 1:
        raise ValueError("Multiple releases claim the canonical MCP tag")
    return found[0] if found else None


def assets(api, release_id):
    result = []
    for page in range(1, 11):
        batch = api.request("GET", f"releases/{release_id}/assets?per_page=100&page={page}")
        result.extend(batch)
        if len(batch) < 100:
            return result
    raise ValueError("Release asset inventory exceeds its bounded page limit")


def require_assets(items, version):
    for name in expected_assets(version):
        matching = [item for item in items if item["name"] == name]
        if len(matching) != 1 or matching[0].get("state") != "uploaded" or matching[0].get("size", 0) <= 0:
            raise ValueError(f"Release is missing a complete asset: {name}")


def prepare(root, api, head):
    empty = {"release_assets_urls": {}, "pending_releases": {}, "release_sha": ""}
    if list((root / ".changepacks").glob("changepack_log_*.json")):
        return empty
    if not SHA.fullmatch(head) or git(root, "rev-parse", "HEAD") != head:
        raise ValueError("Release preparation must run from the workflow source commit")
    if api.request("GET", "git/ref/heads/main")["object"]["sha"] != head:
        return empty
    version = version_at(root)
    if version_at(root, head) != version:
        raise ValueError("Workspace release version has uncommitted changes")
    crate = tomllib.loads((root / SERVER).read_text(encoding="utf-8"))
    if crate["package"].get("version") != {"workspace": True}:
        raise ValueError("The MCP crate must inherit the workspace release version")
    tag = PREFIX + version
    release = find_release(api, tag)
    if release and not release["draft"]:
        require_assets(assets(api, release["id"]), version)
        return empty
    source = tag_source(root, api, tag, version, head)
    if release is None:
        workspace = find_release(api, f"devup-mcp(Cargo.toml)@{version}")
        notes = {"body": workspace["body"]} if workspace and workspace.get("body") else api.request("POST", "releases/generate-notes", {"tag_name": tag, "target_commitish": source})
        release = api.request("POST", "releases", {
            "tag_name": tag, "target_commitish": source, "name": tag,
            "body": notes["body"], "draft": True, "make_latest": "false",
        })
    if release["tag_name"] != tag or not release["draft"]:
        raise ValueError("Expected the canonical MCP draft release")
    return {"release_assets_urls": {SERVER: release["upload_url"]},
            "pending_releases": {SERVER: receipt(release, version, source)}, "release_sha": source}


def bound_release(api, saved):
    if type(saved.get("releaseId")) is not int or saved["releaseId"] <= 0 or saved.get("tagName") != PREFIX + saved["version"] or not SHA.fullmatch(saved["sourceSha"]):
        raise ValueError("Invalid MCP release receipt")
    release = api.request("GET", f"releases/{saved['releaseId']}")
    if release["tag_name"] != saved["tagName"] or release["target_commitish"] != saved["sourceSha"]:
        raise ValueError("Release identity changed after preparation")
    if resolve_tag(api, saved["tagName"]) != saved["sourceSha"]:
        raise ValueError("Release tag changed after preparation")
    return release


def upload(api, saved, file):
    release = bound_release(api, saved)
    if file.name not in expected_assets(saved["version"]) or not file.is_file() or not 0 < file.stat().st_size <= 256 * 1024 * 1024:
        raise ValueError("Unexpected, empty or oversized release asset")
    with file.open("rb") as stream:
        digest = "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()
    existing = [item for item in assets(api, release["id"]) if item["name"] == file.name]
    if existing:
        if len(existing) == 1 and existing[0].get("state") == "starter" and existing[0].get("size") == 0 and release["draft"]:
            api.request("DELETE", f"releases/assets/{existing[0]['id']}")
        elif len(existing) != 1 or existing[0].get("state") != "uploaded" or existing[0].get("size") != file.stat().st_size or existing[0].get("digest") != digest:
            raise ValueError("Existing release asset differs; no asset was overwritten")
        else:
            return
    if not release["draft"]:
        raise ValueError("Refusing to add assets to an already published release")
    uploaded = api.upload(release, file)
    if uploaded.get("digest") != digest or uploaded.get("size") != file.stat().st_size or uploaded.get("state") != "uploaded":
        raise ValueError("Uploaded release asset failed its size/digest verification")


def finalize(api, saved):
    release = bound_release(api, saved)
    require_assets(assets(api, release["id"]), saved["version"])
    if release["draft"]:
        api.request("PATCH", f"releases/{release['id']}", {"draft": False, "make_latest": "true"})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["prepare", "upload", "finalize"])
    parser.add_argument("--file", type=Path)
    args = parser.parse_args()
    api = GitHub(os.environ["GITHUB_REPOSITORY"], os.environ["GH_TOKEN"])
    if args.action == "prepare":
        outputs = prepare(Path.cwd(), api, os.environ["GITHUB_SHA"])
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
            for key, value in outputs.items():
                out.write(f"{key}={value if isinstance(value, str) else json.dumps(value, separators=(',', ':'))}\n")
        print("MCP release pending" if outputs["pending_releases"] else "No MCP release delivery needed")
    else:
        saved = json.loads(os.environ["PENDING_RELEASES"])[SERVER]
        if args.action == "upload":
            if args.file is None:
                parser.error("upload requires --file")
            upload(api, saved, args.file)
        else:
            finalize(api, saved)


if __name__ == "__main__":
    main()
