from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from types import MappingProxyType
from typing import Any, Mapping, Optional, Tuple, Union


class FailureReason(str, Enum):
    TIMED_OUT = "timed_out"
    SESSION_EXITED = "session_exited"
    CANCELLED = "cancelled"
    LOCATOR_NO_MATCH = "locator_no_match"
    LOCATOR_AMBIGUOUS = "locator_ambiguous"
    UNEXPECTED_MATCH = "unexpected_match"
    MATCH_NOT_ACTIONABLE = "match_not_actionable"
    SCALAR_MISMATCH = "scalar_mismatch"
    SNAPSHOT_MISMATCH = "snapshot_mismatch"
    EMULATOR_FAULT = "emulator_fault"
    INTERNAL_FAILURE = "internal_failure"


class FailureArtifactStatus(str, Enum):
    WRITTEN = "written"
    PARTIAL = "partial"
    FAILED = "failed"


def _mapping(value: object) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        return MappingProxyType({})
    return MappingProxyType(dict(value))


def _optional_string(value: object) -> Optional[str]:
    return value if isinstance(value, str) else None


def _enum_or_string(enum_type, value: object):
    if not isinstance(value, str):
        return ""
    try:
        return enum_type(value)
    except ValueError:
        return value


@dataclass(frozen=True)
class FailureDetails:
    schema_version: int
    signature: str
    operation: Mapping[str, Any]
    reason: Union[FailureReason, str]
    summary: str
    locator: Optional[Mapping[str, Any]] = None
    comparison: Optional[Mapping[str, Any]] = None
    evaluation_transitions: Tuple[Mapping[str, Any], ...] = ()
    recent_operations: Tuple[Mapping[str, Any], ...] = ()
    terminal: Optional[Mapping[str, Any]] = None
    process: Optional[Mapping[str, Any]] = None
    runtime: Optional[Mapping[str, Any]] = None
    recording: Optional[Mapping[str, Any]] = None
    hints: Tuple[Mapping[str, Any], ...] = ()
    context: Mapping[str, str] = MappingProxyType({})
    truncated: bool = False

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "FailureDetails":
        def optional_mapping(name: str) -> Optional[Mapping[str, Any]]:
            item = value.get(name)
            return _mapping(item) if isinstance(item, Mapping) else None

        def mapping_tuple(name: str) -> Tuple[Mapping[str, Any], ...]:
            items = value.get(name)
            if not isinstance(items, (list, tuple)):
                return ()
            return tuple(_mapping(item) for item in items if isinstance(item, Mapping))

        raw_context = value.get("context")
        context = (
            MappingProxyType(
                {
                    str(key): item
                    for key, item in raw_context.items()
                    if isinstance(item, str)
                }
            )
            if isinstance(raw_context, Mapping)
            else MappingProxyType({})
        )
        schema_version = value.get("schema_version")
        return cls(
            schema_version=(
                schema_version
                if isinstance(schema_version, int)
                and not isinstance(schema_version, bool)
                else 0
            ),
            signature=_optional_string(value.get("signature")) or "",
            operation=_mapping(value.get("operation")),
            reason=_enum_or_string(FailureReason, value.get("reason")),
            summary=_optional_string(value.get("summary")) or "",
            locator=optional_mapping("locator"),
            comparison=optional_mapping("comparison"),
            evaluation_transitions=mapping_tuple("evaluation_transitions"),
            recent_operations=mapping_tuple("recent_operations"),
            terminal=optional_mapping("terminal"),
            process=optional_mapping("process"),
            runtime=optional_mapping("runtime"),
            recording=optional_mapping("recording"),
            hints=mapping_tuple("hints"),
            context=context,
            truncated=value.get("truncated") is True,
        )


@dataclass(frozen=True)
class FailureArtifactRef:
    status: Union[FailureArtifactStatus, str]
    directory: str
    manifest: Optional[str] = None
    report: Optional[str] = None
    screen_text: Optional[str] = None
    screen_svg: Optional[str] = None
    recording: Optional[str] = None
    errors: Tuple[str, ...] = ()

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "FailureArtifactRef":
        raw_errors = value.get("errors")
        errors = (
            tuple(item for item in raw_errors if isinstance(item, str))
            if isinstance(raw_errors, (list, tuple))
            else ()
        )
        return cls(
            status=_enum_or_string(FailureArtifactStatus, value.get("status")),
            directory=_optional_string(value.get("directory")) or "",
            manifest=_optional_string(value.get("manifest")),
            report=_optional_string(value.get("report")),
            screen_text=_optional_string(value.get("screen_text")),
            screen_svg=_optional_string(value.get("screen_svg")),
            recording=_optional_string(value.get("recording")),
            errors=errors,
        )
