#!/usr/bin/env python3
"""Generate the pydantic-v2 lax-validation expectation table used by crates/patchops-core/src/ops/args.rs tests.

Run with the legacy venv (pydantic 2.x):   bench/.work/legacy/.venv/bin/python tools/gen_pydantic_table.py
It validates every raw JSON text below (parsed with `json.loads`, as FastAPI's `request.json()` does, then
`validate_python` - exactly the legacy path) against the field types the legacy models use (bool, int with ge/le,
str, `str|int|float|bool`) and prints Rust table rows. Paste the output into the `PYDANTIC_TABLE` const.
"""
import json
import sys
from typing import Annotated

from pydantic import BaseModel, Field


class M(BaseModel):
    b: bool
    i: Annotated[int, Field(ge=-(2**63), le=2**63 - 1)]
    s: str
    u: str | int | float | bool


INPUTS = [
    "true", "false", "0", "1", "2", "-1", "0.0", "1.0", "0.5", "-0.0", "1e0", "1e2", '""', '"0"', '"1"', '"t"', '"T"', '"F"',
    '"true"', '"TRUE"', '"True"', '"tRuE"', '"false"', '"y"', '"Y"', '"n"', '"no"', '"NO"', '"yes"', '"on"', '"ON"', '"off"',
    '"OFF"', '"maybe"', '" true"', '"true "', '" 1"', '"1 "', '"\\ttrue"', '"\\ntrue"', '"\\u00a0true"', '"1_0"', '"01"', '"00"',
    '"+1"', '"-0"', '"\\uff11"', '"tru"', "null", "[]", "{}", "[1]", '"80"', '"80.0"', '" 80 "', '"8_0"', '"+80"', '"-80"',
    '"80.5"', '"1e2"', '"0x50"', '"80\\n"', '"\\u0668\\u0660"', "80.0", "8e1", "80.5", "9223372036854775807", "9223372036854775808",
    "-9223372036854775809", "1e19", "1e400", "-1e400", '"9223372036854775808"', '"99999999999999999999999999999999999999999"',
    '"inf"', '"nan"', '"1e3"', '"1_000"', '"1__0"', '"_1"', '"1_"', '"\\u2003 7 \\u2003"', '"7\\u000b"', '"7\\u001c"', "-0", "100000000000000000000",
    '"1.0e2"', '".5e1"', '"5."', '"\\uff15"', "12345678901234567890123", "123456789.12345679", "1.5e300", "5e-324", "-5",
    '"5.0"', '"5.00"', '"+5.0"', '"-5.0"', '".0"', '"0."', '"1_0.0"', '"1.0_0"', '"5.0 "', '"1.0"', '"0.0"', '"-0.0"', '"1.5"', '"1.0e0"',
    '"\\u00a05"', '"5\\u0085"', '"5\\ufeff"', '"١"', '"--5"', '"+-5"', '"5-"', '" "', '"+"', '"-"', '"_"', '"0_0"', '"0_1"', '"00_1"',
]


def fmt(v):
    if isinstance(v, bool):
        return "B:" + ("True" if v else "False")
    if isinstance(v, int):
        return "I:" + str(v)
    if isinstance(v, float):
        return "F:" + repr(v)
    return "S:" + v


def one(raw, field):
    try:
        d = json.loads(raw)
    except Exception:
        return "BADJSON"
    try:
        r = M.model_validate({"b": True, "i": 0, "s": "", "u": "", field: d})
    except Exception:
        return "ERR"
    return fmt(getattr(r, field))


def rs(s):
    """A Rust string literal: printable ASCII as is, everything else as \\u{..}."""
    out = []
    for ch in s:
        if ch in '"\\':
            out.append("\\" + ch)
        elif " " <= ch <= "~":
            out.append(ch)
        else:
            out.append("\\u{%x}" % ord(ch))
    return '"' + "".join(out) + '"'


for raw in INPUTS:
    print(f"    ({rs(raw)}, {rs(one(raw, 'b'))}, {rs(one(raw, 'i'))}, {rs(one(raw, 's'))}, {rs(one(raw, 'u'))}),")
