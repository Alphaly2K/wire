#!/usr/bin/env python3
"""Compatibility entry for the architecture-specific performance driver."""
import sys

if __package__:
    from .perf import v2 as _driver
else:
    from perf import v2 as _driver

if __name__ == '__main__':
    _driver.main()
else:
    sys.modules[__name__] = _driver
