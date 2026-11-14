import js from '@eslint/js';
import tsParser from '@typescript-eslint/parser';
import tsPlugin from '@typescript-eslint/eslint-plugin';
import svelte from 'eslint-plugin-svelte';
import prettier from 'eslint-config-prettier';

export default [
  js.configs.recommended,
  ...svelte.configs['flat/recommended'],
  prettier,
  {
    files: ['**/*.ts', '**/*.svelte'],
    plugins: {
      '@typescript-eslint': tsPlugin,
    },
    rules: {
      ...tsPlugin.configs.recommended.rules,
      // Base no-unused-vars doesn't understand TS type-signature
      // parameter names (e.g. an interface method's param); the
      // typescript-eslint version does.
      'no-unused-vars': 'off',
    },
  },
  {
    files: ['**/*.ts'],
    languageOptions: {
      parser: tsParser,
    },
  },
  {
    files: ['**/*.svelte'],
    languageOptions: {
      parserOptions: {
        parser: tsParser,
        extraFileExtensions: ['.svelte'],
      },
    },
  },
  {
    ignores: ['build/', '.svelte-kit/', 'node_modules/', 'src-tauri/target/', 'src-tauri/gen/'],
  },
];
