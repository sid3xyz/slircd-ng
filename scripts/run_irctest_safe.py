#!/usr/bin/env python3
"""
Safe test runner for irctest that prevents RAM exhaustion.

Runs individual test files in isolated processes with guaranteed cleanup,
using cgroups-based memory limits to prevent OOM.

Usage:
    ./run_irctest_safe.py [--discover] [--output report.txt] [TEST_FILE ...]
    
    --discover          Discover and list all test files (don't run)
    --output FILE       Write results to FILE (default: stdout)
    TEST_FILE ...       Run specific test files (default: all)

Environment:
    REPO_ROOT           Repository root (auto-detected)
    IRCTEST_ROOT        irctest root (default: REPO_ROOT/slirc-irctest)
    SLIRCD_BIN          slircd binary path (default: REPO_ROOT/target/release/slircd)
    MEM_MAX             Memory limit per test (default: 4G)
    SWAP_MAX            Swap limit per test (default: 0)
    TIMEOUT_PER_TEST    Timeout per test in seconds (default: 300)
"""

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Optional


class SafeTestRunner:
    def __init__(self):
        # Auto-detect repository root
        script_dir = Path(__file__).parent
        self.repo_root = script_dir.parent
        
        # Configuration from environment or defaults
        self.irctest_root = Path(os.environ.get(
            "IRCTEST_ROOT",
            self.repo_root / "slirc-irctest"
        ))
        self.slircd_bin = Path(os.environ.get(
            "SLIRCD_BIN",
            self.repo_root / "target" / "release" / "slircd"
        ))
        self.mem_max = os.environ.get("MEM_MAX", "4G")
        self.swap_max = os.environ.get("SWAP_MAX", "0")
        self.timeout_per_test = int(os.environ.get("TIMEOUT_PER_TEST", "300"))
        self.safe_runner = script_dir / "irctest_safe.sh"
        
        # Results tracking
        self.passed: list[str] = []
        self.failed: list[str] = []
        self.skipped: list[str] = []
        self.errors: dict[str, str] = {}
        
        self._validate_setup()
    
    def _validate_setup(self) -> None:
        """Validate that all required files and binaries exist."""
        errors = []
        
        if not self.irctest_root.is_dir():
            errors.append(f"IRCTEST_ROOT not found: {self.irctest_root}")
        
        if not self.slircd_bin.exists():
            errors.append(f"SLIRCD_BIN not found or not executable: {self.slircd_bin}")
        
        if not self.safe_runner.exists():
            errors.append(f"irctest_safe.sh not found: {self.safe_runner}")
        
        if errors:
            for err in errors:
                print(f"ERROR: {err}", file=sys.stderr)
            sys.exit(2)
    
    def discover_tests(self) -> list[Path]:
        """Discover all test files in irctest/server_tests/."""
        server_tests = self.irctest_root / "irctest" / "server_tests"
        if not server_tests.is_dir():
            print(f"ERROR: No server_tests directory: {server_tests}", file=sys.stderr)
            return []
        
        tests = sorted(server_tests.glob("*.py"))
        # Filter out __init__.py and helper modules
        tests = [t for t in tests if t.name != "__init__.py" and not t.name.startswith("_")]
        return tests
    
    def run_test(self, test_file: Path) -> tuple[str, Optional[str]]:
        """
        Run a single test file with memory limits and timeout.
        
        Returns (status, error_msg) where status is one of: "PASS", "FAIL", "SKIP", "ERROR"
        """
        test_path_relative = test_file.relative_to(self.irctest_root)
        
        print(f"\n{'='*70}")
        print(f"Running: {test_path_relative}")
        print(f"{'='*70}")
        
        try:
            # Pre-cleanup: kill any lingering slircd from previous runs
            self._cleanup_lingering_slircd()
            time.sleep(0.2)
            
            # Build command
            env = os.environ.copy()
            env["SLIRCD_BIN"] = str(self.slircd_bin)
            env["MEM_MAX"] = self.mem_max
            env["SWAP_MAX"] = self.swap_max
            env["KILL_SLIRCD"] = "1"
            
            cmd = [
                "bash",
                str(self.safe_runner),
                str(test_path_relative)
            ]
            
            # Run with timeout
            try:
                result = subprocess.run(
                    cmd,
                    cwd=self.irctest_root,
                    env=env,
                    timeout=self.timeout_per_test,
                    capture_output=False,
                    text=True
                )
            except subprocess.TimeoutExpired:
                error_msg = f"Test timeout after {self.timeout_per_test}s"
                print(f"\n[TIMEOUT] {error_msg}", file=sys.stderr)
                return ("ERROR", error_msg)
            
            # Post-cleanup: ensure all slircd processes are killed
            self._cleanup_lingering_slircd()
            time.sleep(0.1)
            
            # Interpret exit code
            if result.returncode == 0:
                print(f"[PASS] {test_path_relative}")
                return ("PASS", None)
            elif result.returncode == 5:
                # pytest exit code 5 = no tests collected
                print(f"[SKIP] {test_path_relative} (no tests collected)")
                return ("SKIP", "No tests found in file")
            else:
                error_msg = f"Exit code {result.returncode}"
                print(f"[FAIL] {test_path_relative}: {error_msg}", file=sys.stderr)
                return ("FAIL", error_msg)
        
        except Exception as e:
            error_msg = str(e)
            print(f"[ERROR] {test_path_relative}: {error_msg}", file=sys.stderr)
            return ("ERROR", error_msg)
        
        finally:
            # Always cleanup
            self._cleanup_lingering_slircd()
    
    def _cleanup_lingering_slircd(self) -> None:
        """Kill any lingering slircd processes."""
        try:
            # Graceful SIGTERM
            subprocess.run(
                ["pkill", "-TERM", "-u", os.environ.get("USER", ""), "-f", "slircd.*config.toml"],
                timeout=1,
                capture_output=True
            )
            time.sleep(0.1)
            
            # Force cleanup with SIGKILL
            subprocess.run(
                ["pkill", "-KILL", "-u", os.environ.get("USER", ""), "-f", "slircd.*config.toml"],
                timeout=1,
                capture_output=True
            )
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass
    
    def run_tests(self, test_files: list[Path]) -> None:
        """Run all tests and track results."""
        if not test_files:
            print("ERROR: No test files to run", file=sys.stderr)
            return
        
        start_time = time.time()
        
        for test_file in test_files:
            status, error = self.run_test(test_file)
            test_name = test_file.name
            
            if status == "PASS":
                self.passed.append(test_name)
            elif status == "FAIL":
                self.failed.append(test_name)
                if error:
                    self.errors[test_name] = error
            elif status == "SKIP":
                self.skipped.append(test_name)
            else:  # ERROR
                self.errors[test_name] = error or "Unknown error"
        
        elapsed = time.time() - start_time
        self._print_summary(elapsed)
    
    def _print_summary(self, elapsed: float) -> None:
        """Print test summary."""
        total = len(self.passed) + len(self.failed) + len(self.skipped)
        pass_pct = (len(self.passed) / total * 100) if total > 0 else 0
        
        print(f"\n{'='*70}")
        print("TEST SUMMARY")
        print(f"{'='*70}")
        print(f"Total:   {total:3d}")
        print(f"Passed:  {len(self.passed):3d} ({pass_pct:.1f}%)")
        print(f"Failed:  {len(self.failed):3d}")
        print(f"Skipped: {len(self.skipped):3d}")
        print(f"Elapsed: {elapsed:.1f}s")
        
        if self.passed:
            print(f"\n✓ PASSED ({len(self.passed)}):")
            for name in sorted(self.passed):
                print(f"  • {name}")
        
        if self.failed:
            print(f"\n✗ FAILED ({len(self.failed)}):")
            for name in sorted(self.failed):
                error = self.errors.get(name, "Unknown")
                print(f"  • {name}: {error}")
        
        if self.skipped:
            print(f"\n⊘ SKIPPED ({len(self.skipped)}):")
            for name in sorted(self.skipped):
                print(f"  • {name}")
        
        print(f"{'='*70}\n")
    
    def save_report(self, output_file: Optional[str]) -> None:
        """Save test results to a file."""
        if not output_file:
            return
        
        try:
            with open(output_file, "w") as f:
                f.write(f"Passed: {len(self.passed)}\n")
                for name in sorted(self.passed):
                    f.write(f"  PASS: {name}\n")
                
                f.write(f"\nFailed: {len(self.failed)}\n")
                for name in sorted(self.failed):
                    error = self.errors.get(name, "Unknown")
                    f.write(f"  FAIL: {name}: {error}\n")
                
                f.write(f"\nSkipped: {len(self.skipped)}\n")
                for name in sorted(self.skipped):
                    f.write(f"  SKIP: {name}\n")
            
            print(f"Report saved to: {output_file}")
        except Exception as e:
            print(f"ERROR: Failed to save report: {e}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Safe test runner for irctest with memory limits and cleanup"
    )
    parser.add_argument(
        "--discover",
        action="store_true",
        help="Discover and list all test files"
    )
    parser.add_argument(
        "--output",
        type=str,
        help="Write results to file"
    )
    parser.add_argument(
        "tests",
        nargs="*",
        help="Test files to run (default: all)"
    )
    
    args = parser.parse_args()
    
    runner = SafeTestRunner()
    
    # Discover tests
    if args.tests:
        # Convert test names to full paths
        test_files = []
        for test_arg in args.tests:
            test_path = Path(test_arg)
            if not test_path.is_absolute():
                # Try relative to irctest_root first
                full_path = runner.irctest_root / test_arg
                if not full_path.exists():
                    # Try as-is
                    full_path = Path(test_arg)
            else:
                full_path = test_path
            
            if full_path.exists():
                test_files.append(full_path)
            else:
                print(f"ERROR: Test file not found: {test_arg}", file=sys.stderr)
                return 1
    else:
        test_files = runner.discover_tests()
    
    # Handle --discover flag
    if args.discover:
        print(f"Discovered {len(test_files)} test files:\n")
        for test_file in test_files:
            rel_path = test_file.relative_to(runner.irctest_root)
            print(f"  {rel_path}")
        return 0
    
    # Run tests
    if not test_files:
        print("ERROR: No test files found", file=sys.stderr)
        return 1
    
    runner.run_tests(test_files)
    runner.save_report(args.output)
    
    # Exit code: 0 if all passed, 1 if any failed
    return 1 if runner.failed else 0


if __name__ == "__main__":
    sys.exit(main())
