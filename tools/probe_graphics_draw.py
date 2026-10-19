#!/usr/bin/env python3
"""Compatibility entry for the native probe in tools.probes.draw."""
import sys

if __package__:
    from .probes import draw as _probe
else:
    from probes import draw as _probe

if __name__ == '__main__':
    _probe.main()
else:
    sys.modules[__name__] = _probe
