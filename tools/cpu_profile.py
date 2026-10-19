#!/usr/bin/env python3
"""Compatibility entry point for the shared CPU profiling tool."""
import sys

if __package__:
    from .perf import profile as _profile
else:
    from perf import profile as _profile

if __name__ == '__main__':
    _profile.main()
else:
    # Preserve imported functions and patches on the original module name.
    sys.modules[__name__] = _profile
