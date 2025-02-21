from pprint import pformat
from typing import Any, Mapping


def _dict_subset_equals(larger: Mapping, smaller: Mapping, path: str) -> None:
    __tracebackhide__ = True  # hide this helper function's traceback from pytest
    for k in smaller.keys():
        new_path = f"{path}[{k}]"
        if k not in larger:
            raise SubsetEqualsException(
                "No key {k} in larger", larger, smaller, new_path
            )
        _subset_equals(larger=larger[k], smaller=smaller[k], path=new_path)


def _list_subset_equals(larger: list, smaller: list, path: str) -> None:
    """
    Example: [1, 2], [2] => true, the smaller is a subset of the larger.
    We do not care about order.
    This is N^2 and I don't care.
    """
    __tracebackhide__ = True  # hide this helper function's traceback from pytest
    for idx, item in enumerate(smaller):
        new_path = f"{path}[{idx}]"
        # try to find a match in the larger set
        found_match = False
        for larger_item in larger:
            try:
                _subset_equals(larger=larger_item, smaller=item, path=new_path)
            except SubsetEqualsException as e:
                pass
            else:
                # success, found a match
                found_match = True
                break

        if found_match:
            continue  # on to the next item in the smaller-set
        else:
            raise SubsetEqualsException(
                "Couldn't find a match for item in larger.",
                larger=larger,
                smaller=item,
                path=new_path,
            )


def _primitive_equals(larger: object, smaller: object, path: str) -> None:
    __tracebackhide__ = True  # hide this helper function's traceback from pytest
    primitives = (int, str, bool, float)
    if any(isinstance(larger, p) and isinstance(smaller, p) for p in primitives):
        if larger != smaller:
            raise SubsetEqualsException("Not equal:", larger, smaller, path)
    else:
        raise SubsetEqualsException(
            "Don't know how to subset-compare this type", larger, smaller, path
        )


class SubsetEqualsException(AssertionError):
    def __init__(self, message: str, larger: Any, smaller: Any, path: str) -> None:
        super().__init__(
            f"{message}\n\n{path}\n\n==Larger==\n{pformat(larger)}\n\n==Smaller==\n{pformat(smaller)}"
        )


def _subset_equals(larger: object, smaller: object, path: str = "") -> None:
    __tracebackhide__ = True  # hide this helper function's traceback from pytest
    if larger is smaller:
        pass  # we good
    elif isinstance(larger, list) and isinstance(smaller, list):
        _list_subset_equals(larger, smaller, path)
    elif isinstance(larger, Mapping) and isinstance(smaller, Mapping):
        _dict_subset_equals(larger, smaller, path)
    else:
        _primitive_equals(larger, smaller, path)


def subset_equals(*, larger: object, smaller: object) -> None:
    """
    in fancy terms,
    Larger = superset
    Smaller = subset
    """
    path = "root_object"
    try:
        _subset_equals(larger=larger, smaller=smaller, path=path)
    except SubsetEqualsException as e:
        raise SubsetEqualsException(
            "Couldn't find a subset", larger, smaller, path
        ) from e
