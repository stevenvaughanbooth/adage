Repository notes learned from earlier sessions in this repo:
- `Bash: find . -type f -name "*.py" | grep -E "(make|class)" | head -20` shows: _make.py, src/attr, src/attr/_make.py, test_make.py, tests/test_make.py
- `Bash: pip install hypothesis -q && python3 -m pytest tests/ -xvs -k "pre_init" 2>&1 | head -150` shows: test_make.py, tests/test_make.py
- `Bash: ls tests/` shows: test_annotations.py
- `Bash: find . -type f -name "*.py" -path "*/test*" | xargs grep -l "Converter" | head -5` shows: test_converters.py, tests/test_converters.py
- `Bash: find src -type f -name "*.py" | head -20` shows: _compat.py, _funcs.py, _make.py, src/attr, src/attr/_compat.py, src/attr/_funcs.py, src/attr/_make.py
- `Bash: find . -type f -name "*.py" | grep -E "(converter|attr)" | head -20` shows: _funcs.py, _make.py, converters.py, src/attr, src/attr/_funcs.py, src/attr/_make.py, src/attr/converters.py
- `Bash: grep -r "cached_property" tests/ --include="*.py"` shows: test_slots.py, tests/test_slots.py
- `Bash: python -m pytest tests/ -x --tb=short 2>&1 | tail -80` shows: test_packaging.py, tests/test_packaging.py
- `Bash: python3 << 'EOF'
import attr
from attr import Converter

def wrapped(_, __, ___) -> float:
    pass

c = Converter(wrapped)
print(f"Re` failed here; what worked: `Bash: PYTHONPATH=src python3 << 'EOF'
import attr
from attr import Converter

def wrapped(_, __, ___) -> float:
    pass

c = Converter(wrapped)
print(f"Return `
- `Bash: find . -type f -name "*.py" | head -20` shows: filters.py, src/attr, src/attr/filters.py, src/attr/validators.py, src/attrs, src/attrs/validators.py, test_validators.py, tests/test_validators.py, validators.py
- `Bash: python -c "
import warnings
warnings.simplefilter('always', DeprecationWarning)

import attr

@attr.s
class C:
    @attr.s(repr_ns='C'` failed here; what worked: `Bash: python -m pip install -e . -q 2>&1 | tail -20 ; Bash: python -W all::DeprecationWarning -c "
import warnings
warnings.simplefilter('always', DeprecationWarning)

import attr

@attr.s
class C:
  `
