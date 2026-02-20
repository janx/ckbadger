#!/usr/bin/env bash

set -euo pipefail

api_routes_changed="${API_ROUTES_CHANGED:-false}"
api_integration_changed="${API_INTEGRATION_CHANGED:-false}"

if [[ "${api_routes_changed}" == "true" && "${api_integration_changed}" != "true" ]]; then
  echo "ERROR: API routes changed but crates/api/tests/api_integration.rs was not updated."
  echo "Add or update integration coverage for API route changes."
  exit 1
fi

echo "Scope guard passed."
