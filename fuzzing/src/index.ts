#!/usr/bin/env node

import { parseCliOptions, defaultFuzzerOptions } from './config';
import { ApiFuzzer } from './runners/api-fuzzer';
import { PageFuzzer } from './runners/page-fuzzer';

type Mode = 'api' | 'page' | 'all';

function parseMode(args: string[]): Mode {
  const modeArg = args.find((arg) => arg.startsWith('--mode='));
  if (modeArg) {
    const mode = modeArg.split('=')[1];
    if (mode === 'api' || mode === 'page' || mode === 'all') {
      return mode;
    }
  }

  if (args.includes('--api')) return 'api';
  if (args.includes('--page')) return 'page';

  return 'all';
}

function printUsage(): void {
  console.log(`
Ckbadger Fuzzing Framework

Usage: npx tsx fuzzing/src/index.ts [options]

Modes:
  --mode=api          Compare ckbadger API with official explorer
  --mode=page         Check page internal consistency
  --mode=all          Run both modes (default)

Options:
  -b, --blocks N      Number of blocks to sample (default: ${defaultFuzzerOptions.blockSampleSize})
  -t, --transactions N  Number of transactions to sample (default: ${defaultFuzzerOptions.txSampleSize})
  -a, --addresses N   Number of addresses to sample (default: ${defaultFuzzerOptions.addressSampleSize})
  -c, --concurrency N Maximum concurrent requests (default: ${defaultFuzzerOptions.concurrency})
  --timeout N         Request timeout in ms (default: ${defaultFuzzerOptions.timeout})
  -o, --output DIR    Output directory for reports (default: ${defaultFuzzerOptions.outputDir})
  -v, --verbose       Enable verbose logging
  --stop-on-error     Stop on first error (default: continue)
  -h, --help          Show this help message

Environment Variables:
  CKBADGER_API_URL    Ckbadger API URL (default: http://localhost:3001/api/v1)
  OFFICIAL_EXPLORER_URL  Official explorer URL (default: https://explorer.nervos.org/api/v1)

Examples:
  # Quick test with 10 samples each
  npx tsx fuzzing/src/index.ts --mode=page -b 10 -t 10 -a 5

  # Full API comparison
  npx tsx fuzzing/src/index.ts --mode=api -b 50 -t 30 -a 20 -v

  # CI mode with minimal samples
  npx tsx fuzzing/src/index.ts --mode=all -b 5 -t 5 -a 3
`);
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);

  if (args.includes('-h') || args.includes('--help')) {
    printUsage();
    process.exit(0);
  }

  const mode = parseMode(args);
  const options = parseCliOptions(args);

  console.log('╔══════════════════════════════════════════════════════════════════════════════╗');
  console.log('║                        CKBADGER FUZZING FRAMEWORK                            ║');
  console.log('╚══════════════════════════════════════════════════════════════════════════════╝');

  try {
    if (mode === 'api' || mode === 'all') {
      const apiFuzzer = new ApiFuzzer(options);
      await apiFuzzer.run();
    }

    if (mode === 'page' || mode === 'all') {
      const pageFuzzer = new PageFuzzer(options);
      await pageFuzzer.run();
    }

    console.log('\n✅ Fuzzing completed successfully!');
  } catch (error) {
    console.error('\n❌ Fuzzing failed:', (error as Error).message);
    process.exit(1);
  }
}

main();
