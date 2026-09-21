import { spawnSync } from 'node:child_process';

const run = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { encoding: 'utf8' });
if (run.error) throw run.error;
if (run.status !== 0) throw new Error(run.stderr);
export const target: string = JSON.parse(run.stdout).target_directory;
