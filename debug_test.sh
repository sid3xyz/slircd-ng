#!/bin/bash
export RUST_LOG=debug
./scripts/irctest_safe.sh slirc-irctest/irctest/server_tests/test_bouncer_multiclient.py::MulticlientTestCase::testQuit > irctest_debug_output.txt 2>&1
