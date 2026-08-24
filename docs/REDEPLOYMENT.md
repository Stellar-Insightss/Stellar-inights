# Redeployment Procedure

When smart contracts are modified and require redeployment, follow these steps to ensure all services are synchronized with the new contract IDs and avoid configuration drift.

## Ordering

1. **Smart Contracts**
2. **Backend Services**
3. **Frontend Services**

This order guarantees that the API is capable of handling the new contract interactions before the frontend attempts to use them.

## Steps

1. **Deploy Contracts and Generate IDs**
   Run the deployment script from the root of the project to deploy the smart contracts and update the single source of truth for contract IDs.
   \`\`\`bash
   ./scripts/deploy-contracts-testnet.sh
   \`\`\`
   This script will write the new contract IDs to \`contracts/contract-ids.json\`.

2. **Commit the Updated Contract IDs**
   Since the generated file \`contracts/contract-ids.json\` is the single source of truth, it must be committed to the repository so the frontend and backend can pick it up during their respective build steps.
   \`\`\`bash
   git add contracts/contract-ids.json
   git commit -m "chore: update contract IDs for new deployment"
   git push origin main
   \`\`\`

3. **Deploy Backend**
   Deploy the backend services. The backend will read the updated \`contracts/contract-ids.json\` during its build or startup.
   \`\`\`bash
   # Follow backend deployment procedures (e.g., triggering a backend CI/CD pipeline or running manual deployment scripts)
   ./scripts/deploy.sh staging <image-tag>
   \`\`\`

4. **Deploy Frontend**
   Deploy the frontend. The frontend application will import the new contract IDs from the JSON file during its build.
   \`\`\`bash
   # Follow frontend deployment procedures (e.g., triggering Vercel/Netlify builds)
   \`\`\`

## CI Drift Check
The repository includes a CI workflow (\`.github/workflows/contract-id-drift-check.yml\`) that ensures the frontend and backend strictly read from the \`contracts/contract-ids.json\` file instead of hardcoding independent contract IDs. This check will fail the build if any configuration drift is detected.
