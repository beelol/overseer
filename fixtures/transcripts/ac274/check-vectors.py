#!/usr/bin/env python3
"""Offline fixture check for the schema keywords used by the frozen installed files.

This is a fixture consistency checker, not a production JSON-Schema implementation.
Unsupported validation keywords fail closed. No provider, package or network calls.
"""
import hashlib
import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parent
ANNOTATIONS = {'$schema', 'title', 'description', 'definitions', 'default', 'enumNames'}
KEYWORDS = {'$ref', 'type', 'enum', 'properties', 'required', 'additionalProperties',
            'oneOf', 'anyOf', 'allOf', 'items', 'minItems', 'maxItems',
            'minLength', 'maxLength', 'pattern', 'minimum', 'maximum', 'format'}


def check(schema, value, document):
    if isinstance(schema, bool):
        return schema
    if set(schema) - ANNOTATIONS - KEYWORDS:
        raise AssertionError(f'Unsupported fixture schema keyword: {set(schema) - ANNOTATIONS - KEYWORDS}')
    if '$ref' in schema:
        target = document
        assert schema['$ref'].startswith('#/')
        for key in schema['$ref'][2:].split('/'):
            target = target[key.replace('~1', '/').replace('~0', '~')]
        if not check(target, value, document):
            return False
    for keyword in ['allOf', 'anyOf', 'oneOf']:
        if keyword in schema:
            count = sum(check(part, value, document) for part in schema[keyword])
            if (keyword == 'allOf' and count != len(schema[keyword])) or (keyword == 'anyOf' and count < 1) or (keyword == 'oneOf' and count != 1):
                return False
    def has_type(name):
        return {'null': value is None, 'boolean': isinstance(value, bool),
                'string': isinstance(value, str), 'object': isinstance(value, dict),
                'array': isinstance(value, list),
                'integer': isinstance(value, int) and not isinstance(value, bool),
                'number': isinstance(value, (int, float)) and not isinstance(value, bool)}[name]
    if 'type' in schema:
        types = schema['type'] if isinstance(schema['type'], list) else [schema['type']]
        if not any(has_type(name) for name in types):
            return False
    if 'enum' in schema and not any(type(value) is type(choice) and value == choice for choice in schema['enum']):
        return False
    if isinstance(value, dict):
        if not set(schema.get('required', [])) <= value.keys():
            return False
        for key, item in value.items():
            rule = schema.get('properties', {}).get(key, schema.get('additionalProperties', True))
            if not check(rule, item, document):
                return False
    if isinstance(value, list):
        if not schema.get('minItems', 0) <= len(value) <= schema.get('maxItems', float('inf')):
            return False
        if 'items' in schema and not all(check(schema['items'], item, document) for item in value):
            return False
    if isinstance(value, str):
        if not schema.get('minLength', 0) <= len(value) <= schema.get('maxLength', float('inf')):
            return False
        if 'pattern' in schema and not re.search(schema['pattern'], value):
            return False
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if not schema.get('minimum', -float('inf')) <= value <= schema.get('maximum', float('inf')):
            return False
        format_name = schema.get('format')
        if format_name == 'int64' and not -(2**63) <= value < 2**63:
            return False
        if format_name in ['uint', 'uint64'] and not 0 <= value < 2**64:
            return False
    return True


def main():
    provenance = json.loads((ROOT / 'provenance.json').read_text())
    for name, digest in provenance['schema_sha256'].items():
        assert hashlib.sha256((ROOT / 'schemas/codex-0.158.0' / name).read_bytes()).hexdigest() == digest, name
    rows = json.loads((ROOT / 'native-vectors.json').read_text())
    assert len({row['name'] for row in rows}) == len(rows)
    requests = responses = 0
    id_schema = json.loads((ROOT / 'schemas/codex-0.158.0/RequestId.json').read_text())
    for row in rows:
        if row['harness'] == 'codex-app' and 'id' in row['request']:
            valid_id = check(id_schema, row['request']['id'], id_schema)
            assert valid_id == (row.get('reject') != 'invalid_native_id'), row['name']
        for direction, field in [('request', 'request_schema'), ('response', 'response_schema')]:
            if field not in row:
                continue
            schema = json.loads((ROOT / 'schemas/codex-0.158.0' / row[field]).read_text())
            value = row['request']['params'] if direction == 'request' else row['response']['result']
            assert check(schema, value, schema), (row['name'], field, value)
            requests += direction == 'request'
            responses += direction == 'response'
        if row['family'] == 'invalid':
            assert 'response' not in row
        if 'reject' in row:
            assert 'response' not in row
    print(f'{len(rows)} unique sanitized vectors; {requests} native requests and {responses} exact response objects match frozen installed schemas; {len(provenance["schema_sha256"])} schema hashes intact')


if __name__ == '__main__':
    main()
