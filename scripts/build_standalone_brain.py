"""Build script for standalone ozy-brain executable using PyInstaller in an isolated environment."""

import os
import subprocess
import sys
from pathlib import Path

def main():
    repo_root = Path(__file__).resolve().parent.parent
    dist_dir = repo_root / "target" / "release"
    dist_dir.mkdir(parents=True, exist_ok=True)
    
    print("[BUILD] Packaging standalone ozy-brain executable...")
    cmd = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--clean",
        "--noconfirm",
        "--distpath",
        str(dist_dir),
        str(repo_root / "ozy-brain.spec"),
    ]
    
    result = subprocess.run(cmd, cwd=repo_root)
    if result.returncode != 0:
        print("[ERROR] PyInstaller packaging failed.", file=sys.stderr)
        return result.returncode
        
    exe_name = "ozy-brain.exe" if os.name == "nt" else "ozy-brain"
    target_exe = dist_dir / exe_name
    if target_exe.exists():
        print(f"[SUCCESS] Standalone executable built at: {target_exe}")
        print(f"          Size: {target_exe.stat().st_size / (1024*1024):.2f} MB")
        return 0
    else:
        print(f"[ERROR] Output executable not found at {target_exe}", file=sys.stderr)
        return 1

if __name__ == "__main__":
    raise SystemExit(main())
