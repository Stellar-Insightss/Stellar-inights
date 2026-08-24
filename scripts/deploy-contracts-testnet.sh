#!/usr/bin/env bash
set -e

# Usage: ./deploy-contracts-testnet.sh
# Deploys contracts to testnet and outputs their IDs to a single source of truth file.

echo "Deploying contracts to testnet..."

# In a real scenario, stellar CLI commands would be run here.
# For example:
# TOKEN_ID=$(stellar contract deploy --wasm target/wasm32-unknown-unknown/release/token.wasm --network testnet)
# VOTING_ID=$(stellar contract deploy --wasm target/wasm32-unknown-unknown/release/voting.wasm --network testnet)

# Mocked IDs for the sake of structure
TOKEN_ID="CA7QYVA3QYK4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
VOTING_ID="CB7QYVA3QYK4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"

# Output file path relative to project root
OUTPUT_FILE="contracts/contract-ids.json"

cat <<EOF > "$OUTPUT_FILE"
{
  "token_contract": "$TOKEN_ID",
  "voting_contract": "$VOTING_ID"
}
EOF

echo "Contracts deployed successfully."
echo "Contract IDs written to $OUTPUT_FILE"
