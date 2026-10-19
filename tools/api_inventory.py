#!/usr/bin/env python3
"""Offline PE import/delay-import/CLR PInvoke inventory plus observed API traces.

Reads game files without executing or modifying them. String candidates are
explicitly unconfirmed; imports and ImplMap rows are declarations, not proof
that a particular path ran. The optional coverage helper checks ABI tables,
not behavioral completeness or optional graphics/input backend capabilities.
"""
# Preserve the original CLI and import surface used by the fixture tools.
from inventory.api import *


if __name__ == '__main__':
    main()
