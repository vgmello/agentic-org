"""Independent Python reader for the checked-in codec-2 byte/hash corpus.

No floating-point conversions. This is test tooling, never a production codec or
an all-input interoperability proof. Run with python3 -B from any directory.
"""
import hashlib
import json
from pathlib import Path


class NumericToken:
    def __init__(self, text):
        self.text = text


def object_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate decoded key")
        result[key] = value
    return result


def refuse_constant(_):
    raise ValueError("invalid JSON constant")


def string(text):
    if any(0xD800 <= ord(c) <= 0xDFFF for c in text):
        raise ValueError("unpaired surrogate")
    return json.dumps(text, ensure_ascii=False, separators=(",", ":"))


def canonical(value):
    if isinstance(value, NumericToken):
        return value.text
    if isinstance(value, str):
        return string(value)
    if isinstance(value, list):
        return "[" + ",".join(map(canonical, value)) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(string(k) + ":" + canonical(value[k]) for k in sorted(value)) + "}"
    return json.dumps(value, separators=(",", ":"))


def decode(text):
    return json.loads(text, parse_int=NumericToken, parse_float=NumericToken,
                      parse_constant=refuse_constant, object_pairs_hook=object_pairs)


def main():
    path = Path(__file__).with_name("selected-codec-2.json")
    vectors = json.loads(path.read_text())
    for case in vectors["valid"]:
        actual = canonical(decode(case["input"])).encode()
        assert actual == case["canonical"].encode()
        assert hashlib.sha256(actual).hexdigest() == case["sha256"]
    for case in vectors["invalid"]:
        try:
            canonical(decode(case))
        except ValueError:
            continue
        raise AssertionError("invalid corpus case admitted")
    print(json.dumps({"status": "PASS", "valid": len(vectors["valid"]),
                      "invalid": len(vectors["invalid"]),
                      "fixture_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                      "scope": "checked-in corpus only"}, sort_keys=True))


if __name__ == "__main__":
    main()
