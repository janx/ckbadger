import { MARKDOWN_ROUTE_PATTERNS } from '@/lib/ai/markdown-route';
import { RAW_ROUTE_PATTERNS } from '@/lib/ai/raw-route';

const RAW_MEDIA_TYPE = 'application/vnd.ckbadger.raw+json';
const MARKDOWN_MEDIA_TYPE = 'text/markdown';

const RAW_DEFAULT_PROFILE = 'default';
const RAW_ROUTE_PROFILES: Record<string, readonly string[]> = {
  '/blocks/{id}': ['default'],
  '/cell/{outpoint}': ['default'],
  '/tx/{hash}': ['default', 'debugger'],
};

export interface AiCapabilities {
  site: {
    name: 'ckbadger';
    apiBase: '/api/v1';
  };
  formatNegotiation: {
    priority: ['query.format', 'path.suffix', 'accept.header'];
    supportedFormats: ['html', 'md', 'raw'];
    markdown: {
      suffix: '.md';
      query: 'format=md';
      accept: typeof MARKDOWN_MEDIA_TYPE;
    };
    raw: {
      suffix: '.raw';
      query: 'format=raw';
      accept: typeof RAW_MEDIA_TYPE;
      profileQuery: 'profile=<name>';
      defaultProfile: typeof RAW_DEFAULT_PROFILE;
    };
  };
  responseHeaders: {
    raw: {
      formatHeader: 'x-ckbadger-format';
      profileHeader: 'x-ckbadger-profile';
      schemaHeader: 'x-ckbadger-schema';
    };
  };
  routes: {
    markdown: readonly string[];
    raw: readonly string[];
  };
  rawProfiles: {
    routes: Record<string, readonly string[]>;
    strictErrors: {
      invalidProfile: 'invalid_profile';
      profileNotSupported: 'profile_not_supported';
    };
    txDebuggerProfile: {
      route: '/tx/{hash}';
      profile: 'debugger';
      payloadPath: 'data.txDebugger.mockTransaction';
      debuggerCommandTemplate: string;
    };
  };
}

export function buildAiCapabilities(origin?: string): AiCapabilities & { origin?: string } {
  return {
    ...(origin ? { origin } : {}),
    site: {
      name: 'ckbadger',
      apiBase: '/api/v1',
    },
    formatNegotiation: {
      priority: ['query.format', 'path.suffix', 'accept.header'],
      supportedFormats: ['html', 'md', 'raw'],
      markdown: {
        suffix: '.md',
        query: 'format=md',
        accept: MARKDOWN_MEDIA_TYPE,
      },
      raw: {
        suffix: '.raw',
        query: 'format=raw',
        accept: RAW_MEDIA_TYPE,
        profileQuery: 'profile=<name>',
        defaultProfile: RAW_DEFAULT_PROFILE,
      },
    },
    responseHeaders: {
      raw: {
        formatHeader: 'x-ckbadger-format',
        profileHeader: 'x-ckbadger-profile',
        schemaHeader: 'x-ckbadger-schema',
      },
    },
    routes: {
      markdown: MARKDOWN_ROUTE_PATTERNS,
      raw: RAW_ROUTE_PATTERNS,
    },
    rawProfiles: {
      routes: RAW_ROUTE_PROFILES,
      strictErrors: {
        invalidProfile: 'invalid_profile',
        profileNotSupported: 'profile_not_supported',
      },
      txDebuggerProfile: {
        route: '/tx/{hash}',
        profile: 'debugger',
        payloadPath: 'data.txDebugger.mockTransaction',
        debuggerCommandTemplate:
          'curl "<url>.raw?profile=debugger" | jq \'.data.txDebugger.mockTransaction\' > mock_tx.json && ckb-debugger --tx-file mock_tx.json --cell-index 0 --cell-type input --script-group-type lock',
      },
    },
  };
}
