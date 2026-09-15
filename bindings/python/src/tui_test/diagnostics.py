from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any, Mapping, Optional, Tuple, Union


class FailureReason(str, Enum):
    COMPLETED = "completed"
    TEST_FAILED = "test_failed"
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
class FailureLocation:
    row: int
    column: int


@dataclass(frozen=True)
class FailureCellMismatch:
    location: FailureLocation
    grapheme: str
    property: str
    operator: str
    expected: str
    actual: str
    reason: str
    resolved: Optional[str] = None

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "FailureCellMismatch":
        return cls(
            location=FailureLocation(**value["location"]),
            grapheme=value["grapheme"],
            property=value["property"],
            operator=value["operator"],
            expected=value["expected"],
            actual=value["actual"],
            reason=value["reason"],
            resolved=value.get("resolved"),
        )


@dataclass(frozen=True)
class FailureLocatorDetails:
    selectors: Tuple[str, ...]
    locations: Tuple[FailureLocation, ...]
    mismatches: Tuple[FailureCellMismatch, ...]
    reason: Optional[str] = None
    stage_index: Optional[int] = None

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "FailureLocatorDetails":
        return cls(
            selectors=tuple(value["selectors"]),
            locations=tuple(FailureLocation(**item) for item in value["locations"]),
            mismatches=tuple(FailureCellMismatch.from_dict(item) for item in value["mismatches"]),
            reason=value.get("reason"),
            stage_index=value.get("stage_index"),
        )


@dataclass(frozen=True)
class FailureComparison:
    kind: str
    expected: Optional[str] = None
    actual: Optional[str] = None


@dataclass(frozen=True)
class FailureDetails:
    schema_version: int
    operation: str
    reason: Union[FailureReason, str]
    summary: str
    locator: Optional[FailureLocatorDetails] = None
    comparison: Optional[FailureComparison] = None
    truncated: bool = False

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "FailureDetails":
        if not isinstance(value["operation"], str) or not isinstance(value["summary"], str):
            raise TypeError("failure operation and summary must be strings")
        locator = value.get("locator")
        comparison = value.get("comparison")
        return cls(
            schema_version=value["schema_version"],
            operation=value["operation"],
            reason=_enum_or_string(FailureReason, value["reason"]),
            summary=value["summary"],
            locator=FailureLocatorDetails.from_dict(locator) if locator is not None else None,
            comparison=FailureComparison(**comparison) if comparison is not None else None,
            truncated=value["truncated"],
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
    report_html: Optional[str] = None
    timeline: Optional[str] = None

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
            report_html=_optional_string(value.get("report_html")),
            timeline=_optional_string(value.get("timeline")),
            screen_text=_optional_string(value.get("screen_text")),
            screen_svg=_optional_string(value.get("screen_svg")),
            recording=_optional_string(value.get("recording")),
            errors=errors,
        )
