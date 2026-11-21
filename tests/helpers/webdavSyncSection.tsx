import { render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { WebdavSyncSection } from "@/components/settings/WebdavSyncSection";
import type { S3SyncSettings, WebDavSyncSettings } from "@/types";

// Fixtures shared by the WebdavSyncSection specs. The specs own the vi.mock calls.
export const baseConfig: WebDavSyncSettings = {
  enabled: true,
  baseUrl: "https://dav.example.com/dav/",
  username: "alice",
  password: "secret",
  remoteRoot: "cc-switch-sync",
  profile: "default",
  autoSync: false,
  status: {},
};

export const s3Config: S3SyncSettings = {
  enabled: true,
  autoSync: true,
  region: "eu-west-1",
  bucket: "my-bucket",
  accessKeyId: "AKIAEXAMPLE",
  secretAccessKey: "s3-secret",
  remoteRoot: "cc-switch-sync",
  profile: "default",
  status: {},
};

export function renderSection(
  config?: WebDavSyncSettings,
  s3?: S3SyncSettings,
) {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <WebdavSyncSection config={config} s3Config={s3} />
    </QueryClientProvider>,
  );
  return { ...view, client };
}
