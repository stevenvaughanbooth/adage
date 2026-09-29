Repository notes learned from earlier sessions in this repo:
- `Bash: find . -type f -name "*.py" | head -20` shows: schema.py, src/marshmallow, src/marshmallow/schema.py, src/marshmallow/utils.py
- `Bash: pip install simplejson -q && python3 -m pytest tests/test_deserialization.py::TestDeserialization::test_constant_field_deserialization` failed here; what worked: `Bash: python3 -m pytest tests/test_deserialization.py -k "constant" -xvs`
- `Bash: find . -type f -name "*.py" | grep -E "(field|datetime)" | head -20` shows: fields.py, src/marshmallow, src/marshmallow/fields.py, test_fields.py, tests/test_fields.py
- `Bash: grep -r "must be declared as a Field instance" tests/ --include="*.py"` shows: test_schema.py, tests/test_schema.py
- `Grep: Constant path=tests output_mode=files_with_matches` shows: test_deserialization.py, test_serialization.py, tests/test_deserialization.py, tests/test_serialization.py
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
- `Read: tests/test_validate.py offset=[244, 260]` failed here; what worked: `Read: tests/test_validate.py offset=244 limit=20`
- `Bash: find . -type f -name "*.py" | grep -E "(validate|validator)" | head -20` shows: src/marshmallow, src/marshmallow/validate.py, test_validate.py, tests/test_validate.py, validate.py
