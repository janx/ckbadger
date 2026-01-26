import * as fs from 'fs';
import * as path from 'path';
import { config } from '../config';
import type { ComparisonResult, PageConsistencyCheck, FuzzingReport, Severity } from '../types';

export class InconsistencyReporter {
  private outputDir: string;

  constructor(outputDir?: string) {
    this.outputDir = outputDir ?? config.output.reportsDir;
  }

  generateReport(
    issues: ComparisonResult[],
    pageIssues: PageConsistencyCheck[] = [],
    mode: FuzzingReport['mode'] = 'all',
    startTime: Date = new Date()
  ): FuzzingReport {
    const endTime = new Date();
    const totalChecks = issues.length + pageIssues.length;
    const failedApiChecks = issues.length;
    const failedPageChecks = pageIssues.filter((p) => !p.isConsistent).length;

    const report: FuzzingReport = {
      startTime: startTime.toISOString(),
      endTime: endTime.toISOString(),
      duration: endTime.getTime() - startTime.getTime(),
      mode,
      config: {
        sampleSize: config.sampling.recentBlocksRange,
        ckbadgerUrl: config.ckbadger.baseUrl,
        officialUrl: config.official.baseUrl,
      },
      summary: {
        totalChecks,
        passed: totalChecks - failedApiChecks - failedPageChecks,
        failed: failedApiChecks + failedPageChecks,
        byEntity: this.groupByEntity(issues),
        bySeverity: this.groupBySeverity(issues),
      },
      issues,
      pageConsistencyIssues: pageIssues.filter((p) => !p.isConsistent),
    };

    return report;
  }

  printToConsole(report: FuzzingReport): void {
    const divider = '='.repeat(80);
    const subDivider = '-'.repeat(80);

    console.log('\n' + divider);
    console.log('FUZZING REPORT');
    console.log(divider);

    console.log(`\nMode: ${report.mode}`);
    console.log(`Duration: ${(report.duration / 1000).toFixed(2)}s`);
    console.log(`Start: ${report.startTime}`);
    console.log(`End: ${report.endTime}`);

    console.log(`\n${subDivider}`);
    console.log('SUMMARY');
    console.log(subDivider);
    console.log(`Total Checks: ${report.summary.totalChecks}`);
    console.log(`Passed: ${report.summary.passed}`);
    console.log(`Failed: ${report.summary.failed}`);

    console.log('\nBy Severity:');
    for (const [severity, count] of Object.entries(report.summary.bySeverity)) {
      const icon = severity === 'critical' ? '🔴' : severity === 'warning' ? '🟡' : '🔵';
      console.log(`  ${icon} ${severity}: ${count}`);
    }

    console.log('\nBy Entity:');
    for (const [entity, count] of Object.entries(report.summary.byEntity)) {
      console.log(`  ${entity}: ${count}`);
    }

    const criticalIssues = report.issues.filter((i) => i.severity === 'critical');
    if (criticalIssues.length > 0) {
      console.log(`\n${subDivider}`);
      console.log('CRITICAL ISSUES');
      console.log(subDivider);

      for (const issue of criticalIssues) {
        console.log(`\n🔴 [${issue.entity}] ${issue.id}`);
        console.log(`   Field: ${issue.field}`);
        console.log(`   Ckbadger: ${JSON.stringify(issue.ckbadger)}`);
        console.log(`   Official: ${JSON.stringify(issue.official)}`);
        console.log(`   Message: ${issue.message}`);
      }
    }

    const pageIssues = report.pageConsistencyIssues ?? [];
    if (pageIssues.length > 0) {
      console.log(`\n${subDivider}`);
      console.log('PAGE CONSISTENCY ISSUES');
      console.log(subDivider);

      for (const issue of pageIssues) {
        console.log(`\n🟡 [${issue.page}]`);
        console.log(`   Field: ${issue.countField}`);
        console.log(`   Count shows: ${issue.countValue}`);
        console.log(`   List has: ${issue.listLength}`);
        if (issue.details) {
          console.log(`   Details: ${issue.details}`);
        }
      }
    }

    console.log('\n' + divider);
  }

  saveToFile(report: FuzzingReport): string {
    if (!fs.existsSync(this.outputDir)) {
      fs.mkdirSync(this.outputDir, { recursive: true });
    }

    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const filename = `fuzzing-${report.mode}-${timestamp}.json`;
    const filepath = path.join(this.outputDir, filename);

    fs.writeFileSync(filepath, JSON.stringify(report, null, 2));
    console.log(`\nReport saved to: ${filepath}`);

    return filepath;
  }

  private groupByEntity(issues: ComparisonResult[]): Record<string, number> {
    const groups: Record<string, number> = {};
    for (const issue of issues) {
      groups[issue.entity] = (groups[issue.entity] ?? 0) + 1;
    }
    return groups;
  }

  private groupBySeverity(issues: ComparisonResult[]): Record<Severity, number> {
    const groups: Record<Severity, number> = { critical: 0, warning: 0, info: 0 };
    for (const issue of issues) {
      groups[issue.severity]++;
    }
    return groups;
  }
}
