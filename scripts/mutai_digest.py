"""Digest helpers backed by the installed Covey binary."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path


def blake3_file(path: Path) -> str:
    result = subprocess.run(
        ["covey", "--json", "digest", "blake3", "--file", str(path)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        stderr = result.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"covey blake3 digest failed: {stderr}")
    payload = json.loads(result.stdout.decode("utf-8"))
    digest = payload["data"]["digest"]
    if not isinstance(digest, str) or not digest.startswith("blake3:"):
        raise RuntimeError(f"covey returned invalid blake3 digest: {digest!r}")
    return digest
