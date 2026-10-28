#!/usr/bin/env python3
# Sample Python file for syntax highlighting and shebang detection

"""
Multi-line triple-quoted docstring
verifying Python multi-line string state caching.
"""

def fibonacci_sequence(limit: int) -> list:
    values = [0, 1]
    while len(values) < limit:
        next_val = values[-1] + values[-2]
        values.append(next_val)
    return values

if __name__ == "__main__":
    result = fibonacci_sequence(10)
    print("Generated sequence:", result, True, 3.14159)
