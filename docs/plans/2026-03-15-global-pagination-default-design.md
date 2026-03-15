# Global Pagination Default Design

## Goal

- Change the default page size for paginated explorer pages to 50 records per page.

## Problem

- Frontend pages currently hardcode multiple page sizes (`20` and `25`) across query calls and pagination UI props.
- API route modules also expose a shared default pagination limit of `20`.
- This splits the behavior across multiple call sites and makes "default page size" a repeated constant instead of one policy.

## Decision

- Introduce one frontend shared constant for the default paginated page size.
- Replace page-level hardcoded `limit` and `pageSize` values with that constant.
- Change the API shared default limit from `20` to `50` so frontend behavior and API defaults remain aligned.

## Alternatives Considered

### Frontend-only replacement

- Pros: smaller code change.
- Cons: API defaults stay at `20`, so routes without explicit `limit` drift from the visible frontend default.

### Per-page local constants

- Pros: low refactor effort in each file.
- Cons: still duplicates policy and makes future changes error-prone.

## Testing

- Update targeted frontend page tests to assert `50` is used by default.
- Update the API shared default limit test to assert `50`.
- Run focused frontend and Rust tests, then broader frontend verification if the targeted tests pass.
