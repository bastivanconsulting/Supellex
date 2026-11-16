import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  {
    rules: {
      // Chargement initial des données au montage (fetch on mount) : les
      // setState ont lieu après await, jamais de façon synchrone dans le
      // corps de l'effect. Pattern standard React pour ce cas.
      "react-hooks/set-state-in-effect": "off",
    },
  },
  // Globally ignore build artifacts
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    // Divers:
    "node_modules/**",
    "*.config.*",
  ]),
]);

export default eslintConfig;
