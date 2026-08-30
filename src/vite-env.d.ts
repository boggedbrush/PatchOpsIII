/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_PATCHOPSIII_TITLEBAR_PLATFORM?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
