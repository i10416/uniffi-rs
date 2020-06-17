/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use anyhow::Result;
use askama::Template;

use crate::interface::*;

// Some config options for it the caller wants to customize the generated python.
// Note that this can only be used to control details of the python *that do not affect the underlying component*,
// sine the details of the underlying component are entirely determined by the `ComponentInterface`.
pub struct Config {
    // No config options yet.
}

impl Config {
    pub fn from(_ci: &ComponentInterface) -> Self {
        Config {
            // No config options yet
        }
    }
}

#[derive(Template)]
#[template(
    ext = "py",
    escape = "none",
    source = r###"
# This file was autogenerated by some hot garbage in the `uniffi` crate.
# Trust me, you don't want to mess with it!

# Common helper code.
#
# Ideally this would live in a separate .py file where it can be unittested etc
# in isolation, and perhaps even published as a re-useable package.
#
# However, it's important that the detils of how this helper code works (e.g. the
# way that different builtin types are passed across the FFI) exactly match what's
# expected by the rust code on the other side of the interface. In practice right
# now that means coming from the exact some version of `uniffi` that was used to
# compile the rust component. The easiest way to ensure this is to bundle the Python
# helpers directly inline like we're doing here.

import ctypes
import enum
import struct
import contextlib

# This is how we find and load the dynamic library provided by the component.
# For now we just look it up by name.
#
# XXX TODO: This will probably grow some magic for resolving megazording in future.
# E.g. we might start by looking for the named component in `libuniffi.so` and if
# that fails, fall back to loading it separately from `lib${componentName}.so`.

def loadIndirect(componentName):
    # XXX TODO: different naming conventions on different platforms.
    return getattr(ctypes.cdll, "libuniffi_{}.dylib".format(componentName))

# This is a helper for safely working with byte buffers returned from the Rust code.
# It's basically a wrapper around a length and a data pointer, corresponding to the
# `ffi_support::ByteBuffer` struct on the rust side.

class RustBuffer(ctypes.Structure):
    _fields_ = [
        ("len", ctypes.c_long),
        ("data", ctypes.POINTER(ctypes.c_char)),
    ]

    @staticmethod
    def alloc(size):
        return _UniFFILib.{{ ci.ffi_bytebuffer_alloc().name() }}(size)

    def free(self):
        return _UniFFILib.{{ ci.ffi_bytebuffer_free().name() }}(self)

    def __str__(self):
        return "RustBuffer(len={}, data={})".format(self.len, self.data[0:self.len])

# Helpers for lifting/lowering primitive data types from/to a bytebuffer.

class RustBufferStream(object):

    def __init__(self, rbuf):
        self.rbuf = rbuf
        self.offset = 0

    @contextlib.contextmanager
    def checked_access(self, numBytes):
        if self.offset + numBytes > self.rbuf.len:
            raise RuntimeError("access past end of rust buffer")
        yield None
        self.offset += numBytes

    def _unpack_from(self, size, format):
        if self.offset + size > self.rbuf.len:
            raise RuntimeError("read past end of rust buffer")
        value = struct.unpack(format, self.rbuf.data[self.offset:self.offset+size])[0]
        self.offset += size
        return value

    def _pack_into(self, size, format, value):
        if self.offset + size > self.rbuf.len:
            raise RuntimeError("write past end of rust buffer")
        # XXX TODO: I feel like I should be able to use `struct.pack_into` here but can't figure it out.
        for i, byte in enumerate(struct.pack(format, value)):
            self.rbuf.data[self.offset + i] = byte
        self.offset += size

    def getByte(self):
        return self._unpack_from(1, ">c")

    def putByte(self, v):
        self._pack_into(1, ">c", v)

    def getDouble(self):
        return self._unpack_from(8, ">d")

    def putDouble(self, v):
        self._pack_into(8, ">d", v)

def liftOptional(rbuf, liftFrom):
    return liftFromOptional(RustBufferStream(rbuf), liftFrom)

def liftFromOptional(buf, liftFrom):
    if buf.getByte() == b"\x00":
        return None
    return liftFrom(buf)

# A ctypes library to expose the extern-C FFI definitions.
# This is an implementation detail which will be called internally by the public API.

_UniFFILib = loadIndirect(componentName="{{ ci.namespace() }}")

{%- for func in ci.iter_ffi_function_definitions() %}
_UniFFILib.{{ func.name() }}.argtypes = (
    {%- for arg in func.arguments() %}
    {{ arg.type_()|decl_c }},
    {%- endfor %}
)
_UniFFILib.{{ func.name() }}.restype = {% match func.return_type() %}{% when Some with (type_) %}{{ type_|decl_c }}{% when None %}None{% endmatch %}
{%- endfor %}

# Public interface members begin here.

{% for e in ci.iter_enum_definitions() %}
class {{ e.name() }}(enum.Enum):
    {% for value in e.values() -%}
    {{ value }} = {{ loop.index }}
    {% endfor -%}
{%- endfor -%}

{%- for rec in ci.iter_record_definitions() %}
class {{ rec.name() }}(object):
    def __init__(self,{% for field in rec.fields() %}{{ field.name() }}{% if loop.last %}{% else %}, {% endif %}{% endfor %}):
        {%- for field in rec.fields() %}
        self.{{ field.name() }} = {{ field.name() }}
        {%- endfor %}

    def __str__(self):
        return "{{ rec.name() }}({% for field in rec.fields() %}{{ field.name() }}={}{% if loop.last %}{% else %}, {% endif %}{% endfor %})".format({% for field in rec.fields() %}self.{{ field.name() }}{% if loop.last %}{% else %}, {% endif %}{% endfor %})

    @classmethod
    def _coerce(cls, v):
        # TODO: maybe we could do a bit of duck-typing here, details TBD
        assert isinstance(v, {{ rec.name() }})
        return v

    @classmethod
    def _lift(cls, rbuf):
        return cls._liftFrom(RustBufferStream(rbuf))

    @classmethod
    def _liftFrom(cls, buf):
        return cls(
            {%- for field in rec.fields() %}
            {{ "buf"|lift_from_py(field.type_()) }}{% if loop.last %}{% else %},{% endif %}
            {%- endfor %}
        )

    @classmethod
    def _lower(cls, v):
        rbuf = RustBuffer.alloc(cls._lowersIntoSize(v))
        cls._lowerInto(v, RustBufferStream(rbuf))
        return rbuf

    @classmethod
    def _lowersIntoSize(cls, v):
        return 0 + \
            {%- for field in rec.fields() %}
            {{ "(v.{})"|format(field.name())|lowers_into_size_py(field.type_()) }}{% if loop.last %}{% else %} + \{% endif %}
            {%- endfor %}

    @classmethod
    def _lowerInto(cls, v, buf):
        {%- for field in rec.fields() %}
        {{ "(v.{})"|format(field.name())|lower_into_py("buf", field.type_()) }}
        {%- endfor %}
{% endfor %}

{% for func in ci.iter_function_definitions() %}
def {{ func.name() }}({% for arg in func.arguments() %}{{ arg.name() }}{% if loop.last %}{% else %}, {% endif %}{% endfor %}):
    {%- for arg in func.arguments() %}
    {{ arg.name()|coerce_py(arg.type_()) }}
    {%- endfor %}
    _retval = _UniFFILib.{{ func.ffi_func().name() }}(
        {%- for arg in func.arguments() %}
        {{ arg.name()|lower_py(arg.type_()) }}{% if loop.last %}{% else %},{% endif %}
        {%- endfor %}
    )
    return {% match func.return_type() %}{% when Some with (return_type) %}{{ "_retval"|lift_py(return_type) }}{% else %}None{% endmatch %}
{% endfor %}

{% for obj in ci.iter_object_definitions() %}
class {{ obj.name() }}(object):
    # XXX TODO: support for multiple constructors...
    {%- for cons in obj.constructors() %}
    def __init__(self, {% for arg in cons.arguments() %}{{ arg.name() }}{% if loop.last %}{% else %}, {% endif %}{% endfor %}):
        {%- for arg in cons.arguments() %}
        {{ arg.name()|coerce_py(arg.type_()) }}
        {%- endfor %}
        self._handle = _UniFFILib.{{ cons.ffi_func().name() }}(
            {%- for arg in cons.arguments() %}
            {{ arg.name()|lower_py(arg.type_()) }}{% if loop.last %}{% else %},{% endif %}
            {%- endfor %}
        )
    {%- endfor %}

    # XXX TODO: destructors or equivalent.

    {%- for meth in obj.methods() %}
    def {{ meth.name() }}(self, {% for arg in meth.arguments() %}{{ arg.name() }}{% if loop.last %}{% else %}, {% endif %}{% endfor %}):
        {%- for arg in meth.arguments() %}
        {{ arg.name()|coerce_py(arg.type_()) }}
        {%- endfor %}
        _retval = _UniFFILib.{{ meth.ffi_func().name() }}(
            self._handle,
            {%- for arg in meth.arguments() %}
            {{ arg.name()|lower_py(arg.type_()) }}{% if loop.last %}{% else %},{% endif %}
            {%- endfor %}
        )
        return {% match meth.return_type() %}{% when Some with (return_type) %}{{ "_retval"|lift_py(return_type) }}{% else %}None{% endmatch %}
    {%- endfor %}
{% endfor %}

__all__ = [
    {%- for e in ci.iter_enum_definitions() %}
    "{{ e.name() }}",
    {%- endfor %}
    {%- for record in ci.iter_record_definitions() %}
    "{{ record.name() }}",
    {%- endfor %}
    {%- for func in ci.iter_function_definitions() %}
    "{{ func.name() }}",
    {%- endfor %}
    {%- for obj in ci.iter_object_definitions() %}
    "{{ obj.name() }}",
    {%- endfor %}
]
"###
)]
pub struct PythonWrapper<'a> {
    _config: Config,
    ci: &'a ComponentInterface,
}
impl<'a> PythonWrapper<'a> {
    pub fn new(_config: Config, ci: &'a ComponentInterface) -> Self {
        Self { _config, ci }
    }
}

mod filters {
    use super::*;
    use std::fmt;

    pub fn decl_c(type_: &TypeReference) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::U32 => "ctypes.c_uint32".to_string(),
            TypeReference::U64 => "ctypes.c_uint64".to_string(),
            TypeReference::Float => "ctypes.c_float".to_string(),
            TypeReference::Double => "ctypes.c_double".to_string(),
            TypeReference::Boolean => "ctypes.c_byte".to_string(),
            TypeReference::Bytes => "RustBuffer".to_string(),
            TypeReference::Enum(_) => "ctypes.c_uint32".to_string(),
            TypeReference::Record(_) => "RustBuffer".to_string(),
            TypeReference::Optional(_) => "RustBuffer".to_string(),
            TypeReference::Object(_) => "ctypes.c_uint64".to_string(),
            _ => panic!("[TODO: decl_c({:?})", type_),
        })
    }

    pub fn coerce_py(
        nm: &dyn fmt::Display,
        type_: &TypeReference,
    ) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::U32
            | TypeReference::U64
            | TypeReference::Float
            | TypeReference::Double
            | TypeReference::Boolean => format!("{} = {}", nm, nm),
            TypeReference::Enum(type_name) => format!("{} = {}({})", nm, type_name, nm),
            TypeReference::Record(type_name) => format!("{} = {}._coerce({})", nm, type_name, nm),
            //TypeReference::Optional(_) => "RustBuffer".to_string(),
            _ => panic!("[TODO: coerce_py({:?})]", type_),
        })
    }

    pub fn lower_py(nm: &dyn fmt::Display, type_: &TypeReference) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::U32
            | TypeReference::U64
            | TypeReference::Float
            | TypeReference::Double
            | TypeReference::Boolean => nm.to_string(),
            TypeReference::Enum(_) => format!("{}.value", nm),
            TypeReference::Record(type_name) => format!("{}._lower({})", type_name, nm),
            TypeReference::Optional(_type) => format!(
                "lowerOptional({}, lambda buf, v: {})",
                nm,
                lower_into_py(&"buf", &"v", type_)?
            ),
            _ => panic!("[TODO: lower_py({:?})]", type_),
        })
    }

    pub fn lowers_into_size_py(
        nm: &dyn fmt::Display,
        type_: &TypeReference,
    ) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::Double => "8".to_string(),
            TypeReference::Record(type_name) => format!("{}._lowersIntoSize({})", type_name, nm),
            _ => panic!("[TODO: lowers_into_size_py({:?})]", type_),
        })
    }
    pub fn lower_into_py(
        nm: &dyn fmt::Display,
        target: &dyn fmt::Display,
        type_: &TypeReference,
    ) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::Double => format!("{}.putDouble({})", target, nm),
            TypeReference::Record(type_name) => {
                format!("{}._lowerInto({}, {})", type_name, nm, target)
            }
            _ => panic!("[TODO: lower_into_py({:?})]", type_),
        })
    }

    pub fn lift_py(nm: &dyn fmt::Display, type_: &TypeReference) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::U32
            | TypeReference::U64
            | TypeReference::Float
            | TypeReference::Double
            | TypeReference::Boolean => format!("{}", nm),
            TypeReference::Enum(type_name) => format!("{}({})", type_name, nm),
            TypeReference::Record(type_name) => format!("{}._lift({})", type_name, nm),
            TypeReference::Optional(type_) => format!(
                "liftOptional({}, lambda buf: {})",
                nm,
                lift_from_py(&"buf", type_)?
            ),
            _ => panic!("[TODO: lift_py({:?})]", type_),
        })
    }

    pub fn lift_from_py(
        nm: &dyn fmt::Display,
        type_: &TypeReference,
    ) -> Result<String, askama::Error> {
        Ok(match type_ {
            TypeReference::Double => format!("{}.getDouble()", nm),
            TypeReference::Record(type_name) => format!("{}._liftFrom({})", type_name, nm),
            _ => panic!("[TODO: lift_from_py({:?})]", type_),
        })
    }
}
