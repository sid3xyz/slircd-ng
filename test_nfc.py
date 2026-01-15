#!/usr/bin/env python3
import unicodedata

# Latin e (U+0065)
latin_e = "e"
print(f"Latin e: {repr(latin_e)} = U+{ord(latin_e):04X}")
print(f"  NFC: {repr(unicodedata.normalize('NFC', latin_e))}")
print(f"  NFD: {repr(unicodedata.normalize('NFD', latin_e))}")

# Cyrillic e (U+0435)
cyrillic_e = "е"
print(f"Cyrillic е: {repr(cyrillic_e)} = U+{ord(cyrillic_e):04X}")
print(f"  NFC: {repr(unicodedata.normalize('NFC', cyrillic_e))}")
print(f"  NFD: {repr(unicodedata.normalize('NFD', cyrillic_e))}")

# Full words
latin = "evan"
cyrillic = "еvan"
print(f"\nLatin 'evan': {repr(latin)}")
print(f"  NFC: {repr(unicodedata.normalize('NFC', latin))}")
print(f"Cyrillic 'еvan': {repr(cyrillic)}")
print(f"  NFC: {repr(unicodedata.normalize('NFC', cyrillic))}")

print(f"\nAre they equal after NFC? {unicodedata.normalize('NFC', latin) == unicodedata.normalize('NFC', cyrillic)}")
