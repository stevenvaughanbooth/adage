Repository notes learned from earlier sessions in this repo:
- `Read: src/attr/_make.py` shows: slot
- `Bash: pip install hypothesis -q && python3 -m pytest tests/ -xvs -k "pre_init" 2>&1 | head -150` shows: test_make.py, tests/test_make.py
- `Read: src/attr/_funcs.py` shows: PY_3_9_PLUS
- `Read: tests/test_converters.py limit=100` shows: pickle, pickle.dumps, pickle.loads, pickled
- `Read: src/attr/converters.py` shows: _AnnotationExtractor, pipe
- `Read: src/attr/_compat.py` shows: PYPY, PY_3_10_PLUS, PY_3_12_PLUS, PY_3_13_PLUS, PY_3_8_PLUS, PyPy, annotation, platform.python_implementation, sys.version_info
- `Bash: PYTHONPATH=src python3 << 'EOF'
import attr

# Check the structure of an attrib
ib = attr.ib(t` shows: _CountingAttr
- `Read: src/attr/validators.py` shows: @attrs, _InValidator, attr.name, attrib, hash, in_options, inst, self.options, slots, validators
- `Bash: ls tests/` shows: test_annotations.py
- `Read: src/attr/filters.py` shows: frozenset
- `Bash: .venv/bin/pytest tests/test_make.py::TestMakeClass -xvs 2>&1 | head -200` shows: test_attr_args
- `Grep: repr_ns output_mode=content type=py` shows: namespace
- `Read: src/attr/_funcs.py offset=397 limit=50` shows: getattr
- `Bash: grep -n "_is_class_var" src/attr/_make.py` shows: annot
- `Read: src/attr/_make.py offset=1017 limit=40` shows: _make_repr
- `Read: src/attr/_make.py offset=535 limit=50` shows: BaseException
- `Read: src/attr/_make.py offset=287 limit=30` shows: _CLASSVAR_PREFIXES
- `Read: src/attr/_make.py offset=1300 limit=100` shows: attr.ib, on_setattr
- `Bash: grep -B 5 "def test_in" tests/test_validators.py | head -30` shows: TestIn_
- `Bash: grep -n "__attrs_pre_init__" src/attr/_make.py` shows: pre_init, pre_init_args
- `Bash: python -m pytest tests/ -x -k "frozen" --tb=short 2>&1 | tail -50` shows: mypy
- `Read: src/attr/_make.py offset=370 limit=150` shows: _get_annotations, _is_class_var
- `Bash: grep -B5 -A10 "repr_ns" tests/test_make.py` shows: C.D, test_repr_fake_qualname
- `Read: src/attr/_make.py` shows: Attribute, NOTHING, a.name, base_attrs, factory, kw_only
- `Read: src/attr/_make.py limit=100` shows: automatically, contextlib, linecache, setters.pipe
- `Bash: grep -r "cached_property" tests/ --include="*.py"` shows: test_slots.py, tests/test_slots.py
- `Read: src/attr/_make.py offset=2180 limit=100` shows: kw_arg, kw_only_args, pre_init_kw_only_args, regular
- `Read: src/attr/converters.py` shows: _AnnotationExtractor, xtr, xtr.get_first_param_type, xtr.get_return_type
- `Bash: python -m pytest tests/ -x --tb=short 2>&1 | tail -80` shows: test_packaging.py, tests/test_packaging.py
- `Bash: pip install -e . -q && python -c "
import attr

def hook(cls, attribs):
    return sorted(attr` shows: question
- `Bash: python3 -c "
import attr

@attr.s
class C:
    x: int = attr.ib(validator=attr.validators.in_(` shows: __hash__
- `Bash: pip install -e . -q && python3 << 'EOF'
import attr

@attr.define
class C:
    x: int = attr.f` shows: attr_dict
- `Bash: grep -n "takes_self\|takes_field" src/attr/_make.py | head -30` shows: c.takes_field, c.takes_self, pipe_converter
- `Bash: python3 << 'EOF'
import functools
import attr

# Test 1: Original issue - property raising Att` shows: nonexistent
- `Bash: grep -n "__setattr__\|__delattr__" src/attr/_make.py` shows: Attached, BaseException.__setattr__, _frozen_delattrs, _frozen_setattrs
- `Bash: python3 << 'EOF'
import functools
import attr

@attr.s(slots=True)
class A:
    x = attr.ib()
` shows: __getattr__, getattr, original_error
- `Read: src/attr/_make.py offset=445 limit=50` shows: Mandatory, a.default, a.init, a.kw_only, anyway, had_default, matters, moved, non-mandatory, own_attrs, signature
- `Bash: find . -type f -name "*.py" | head -20` shows: filters.py, src/attr, src/attr/filters.py, src/attr/validators.py, src/attrs, src/attrs/validators.py, test_validators.py, tests/test_validators.py, validators.py
- `Read: src/attr/_make.py offset=2670 limit=100` shows: .get_first_param_type, self.__call__, self._first_param_type, self._takes_both, self._takes_field, self._takes_instance, self._takes_only_value, self.converter, self.takes_field, self.takes_self
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
