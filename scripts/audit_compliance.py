#!/usr/bin/env python3
import os
import subprocess
import sys
import glob
from datetime import datetime

WORKSPACE_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
SLIRCD_DIR = os.path.join(WORKSPACE_ROOT, "slircd-ng")
IRCTEST_DIR = os.path.join(WORKSPACE_ROOT, "irctest")
REPORT_FILE = os.path.join(SLIRCD_DIR, "docs", "COMPLIANCE_REPORT.md")
RUNNER_SCRIPT = os.path.join(SLIRCD_DIR, "scripts", "run_irctest.sh")

def main():
    print(f"Starting compliance audit...")

    # Find test files
    test_files = sorted(glob.glob(os.path.join(IRCTEST_DIR, "irctest", "server_tests", "*.py")))
    # Also include subdirectories if needed, but let's start with top-level files

    results = []

    # Build once
    print("Building slircd-ng...")
    subprocess.run(["cargo", "build", "--release"], cwd=SLIRCD_DIR, check=True)

    for test_file in test_files:
        test_name = os.path.basename(test_file)
        rel_path = os.path.relpath(test_file, IRCTEST_DIR)

        print(f"Running {test_name}...")

        start_time = datetime.now()

        # Run the script with the test file as argument
        # We use the script because it handles server lifecycle
        env = os.environ.copy()
        env["SKIP_BUILD"] = "1"
        env["TIMEOUT"] = "30" # 30s timeout per test file
        proc = subprocess.run(
            [RUNNER_SCRIPT, rel_path],
            cwd=SLIRCD_DIR,
            capture_output=True,
            text=True,
            env=env
        )

        duration = (datetime.now() - start_time).total_seconds()
        success = proc.returncode == 0

        # Extract failure details if any
        details = ""
        if not success:
            # Try to find the failure summary in stdout/stderr
            lines = proc.stdout.splitlines()
            for line in lines:
                if "FAILED" in line or "ERROR" in line:
                    details += line + "\n"
            if not details:
                details = "See logs for details"

        results.append({
            "name": test_name,
            "path": rel_path,
            "success": success,
            "duration": duration,
            "details": details.strip()
        })

        status = "PASS" if success else "FAIL"
        print(f"  -> {status} ({duration:.2f}s)")

    # Generate Report
    with open(REPORT_FILE, "w") as f:
        f.write(f"# slircd-ng RFC Compliance Report\n\n")
        f.write(f"**Date:** {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
        f.write(f"**Total Tests:** {len(results)}\n")
        passed = sum(1 for r in results if r['success'])
        f.write(f"**Passed:** {passed}\n")
        f.write(f"**Failed:** {len(results) - passed}\n\n")

        f.write("| Test Module | Status | Duration | Details |\n")
        f.write("|-------------|--------|----------|---------|\n")

        for r in results:
            status_icon = "✅" if r['success'] else "❌"
            details = r['details'].replace("\n", "<br>")
            # Truncate details if too long
            if len(details) > 100:
                details = details[:97] + "..."
            f.write(f"| `{r['name']}` | {status_icon} | {r['duration']:.2f}s | {details} |\n")

    print(f"Report generated at {REPORT_FILE}")

if __name__ == "__main__":
    main()
