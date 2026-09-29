Repository notes learned from earlier sessions in this repo:
- `Read: src/marshmallow/fields.py limit=100` shows: marshmallow
- `Bash: pip install pytest -q && python3 -m pytest tests/test_deserialization.py::TestDeserialization:` shows: simplejson
- `Read: src/marshmallow/schema.py offset=650 limit=15` shows: getter
- `Read: src/marshmallow/utils.py` shows: issubclass
- `Read: tests/test_schema.py limit=600 offset=540` shows: .dump, BadSchema, age
- `Grep: def test.*constant path=tests output_mode=content context=3` shows: test_constant_field_deserialization
- `Read: pyproject.toml` shows: Too
- `Grep: def deserialize path=src/marshmallow/fields.py output_mode=content context=20` shows: ValidationError
- `Bash: find . -type f -name "*.py" | grep -E "(field|datetime)" | head -20` shows: fields.py, src/marshmallow, src/marshmallow/fields.py, test_fields.py, tests/test_fields.py
- `Bash: grep -r "timestamp" tests/ --include="*.py" | grep -i "def test\|assert"` shows: fmt, test_deserialization.py, test_serialization.py, test_timestamp_field_deserialization, tests/test_deserialization.py, tests/test_serialization.py
- `Bash: python -m pytest tests/test_schema.py::test_fields_must_be_declared_as_instances -xvs` shows: UserSchema, pytz
- `Read: src/marshmallow/schema.py offset=795 limit=25` shows: _do_load
- `Read: src/marshmallow/schema.py` shows: Nested, base.FieldABC, field_name
- `Read: src/marshmallow/utils.py offset=180 limit=30` shows: dt.timezone.utc
- `Read: src/marshmallow/fields.py offset=1996 limit=100` shows: self.load_default
- `Read: tests/test_validate.py limit=100` shows: example.com, invalid_url, valid_url
- `Read: src/marshmallow/validate.py` shows: //localhost, Fill, Hostname, URLS, absent, ftp, localhost, regex.search, value.replace, value.startswith
- `Read: src/marshmallow/fields.py offset=488 limit=200` shows: Same, copy.copy, nested_field
- `Read: src/marshmallow/fields.py offset=115 limit=120` shows: allow_none, dump, dump_default, load_default
- `Read: src/marshmallow/fields.py` shows: AwareDateTime, NaiveDateTime, deserialized, marshmallow.exceptions
- `Grep: from_timestamp|from_timestamp_ms path=src/marshmallow/utils.py output_mode=content` shows: dt.datetime
- `Bash: python3 -c "
from marshmallow import Schema, fields

class MySchema(Schema):
    foo = fields.Constant(None)

try:
    result = MySche` failed here; what worked: `Bash: ls -la | grep -E "setup|requirements|pyproject" ; Bash: pip install -e . -q ; Bash: python3 -c "
from marshmallow import Schema, fields

class MySchema(Schema):
    foo = fields.Constant(None)

`
- `Read: tests/test_deserialization.py offset=540 limit=50` shows: default_timezone, fields.AwareDateTime, fields.NaiveDateTime
- `Read: src/marshmallow/fields.py limit=100` shows: Schema, ValidationError, _InternalT, load_default, missing_, typing.Any, typing.Mapping
- `Read: src/marshmallow/schema.py offset=1031 limit=30` shows: Did, Ignore, checker, dead, field_obj, field_obj.__name__, issubclass, mean, perspective, unsupported
- `Bash: grep -A 20 "class.*Function.*Test\|def test.*function" tests/test_deserialization.py | head -5` shows: test_function_field_deserialization_is_noop_by_default
- `Bash: grep -B 5 -A 5 "must be declared as a Field instance" tests/test_schema.py` shows: test_bind_field_does_not_swallow_typeerror, test_fields_must_be_declared_as_instances
- `Read: tests/test_validate.py offset=208 limit=50` shows: //example.com, HTTP, test_url_accepts_valid_file_urls, test_url_rejects_invalid_file_urls, test_url_relative_and_custom_schemes, url
- `Grep: file://|FILE://|case|insensitive path=tests/test_validate.py output_mode=content` shows: ///home/user/documents/test.pdf, ///tmp/test, 20file.txt, home, home/user, home/user/documents, localhost/tmp, test.pdf
- `Grep: class Constant path=src/marshmallow/fields.py output_mode=content context=30` shows: .__init__, Field, Unpack, _BaseFieldKwargs, _ContantT, deserialize, dump_only, load_only, self.constant, self.dump_default, self.load_default
- `Read: src/marshmallow/fields.py offset=332 limit=150` shows: Deserialize, Field-specific, Schema.load, _miss, attribute/key, fields.Str, marshmallow.Schema.load, schema, self._deserialize, self._validate, self._validate_missing, self.allow_none
