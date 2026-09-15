import { EventEmitter } from 'node:events';
import { setTimeout as sleep } from 'node:timers/promises';
import { randomUUID } from 'node:crypto';

const DEFAULT_CONCURRENCY = 4;
const DEFAULT_RETRIES = 2;
const BACKOFF = [100, 400, 1600];

let started = 0;
let finished = 0;
let failed = 0;

export class Job {
	#id = randomUUID();
	#attempts = 0;
	#result;
	#error;

	constructor(name, run, { priority = 0, retries = DEFAULT_RETRIES, timeout = 0 } = {}) {
		this.name = name;
		this.run = run;
		this.priority = priority;
		this.retries = retries;
		this.timeout = timeout;
		this.state = 'queued';
	}

	get id() {
		return this.#id;
	}

	get attempts() {
		return this.#attempts;
	}

	get result() {
		if (this.state !== 'done') throw new Error(`${this.name} is ${this.state}`);
		return this.#result;
	}

	get error() {
		return this.#error;
	}

	async execute(signal) {
		this.#attempts += 1;
		this.state = 'running';
		try {
			this.#result = this.timeout > 0 ? await withTimeout(this.run(signal), this.timeout, signal) : await this.run(signal);
			this.state = 'done';
			return true;
		} catch (error) {
			this.#error = error;
			this.state = this.#attempts <= this.retries ? 'queued' : 'failed';
			return false;
		}
	}
}

export class Scheduler extends EventEmitter {
	#queue = [];
	#running = new Set();
	#concurrency;
	#controller = new AbortController();
	#idle = null;
	#wake = null;

	constructor({ concurrency = DEFAULT_CONCURRENCY } = {}) {
		super();
		this.#concurrency = concurrency;
	}

	get size() {
		return this.#queue.length + this.#running.size;
	}

	add(name, run, options) {
		const job = new Job(name, run, options);
		this.#queue.push(job);
		this.#queue.sort((a, b) => b.priority - a.priority);
		this.emit('queued', job);
		this.#wake?.();
		return job;
	}

	async *drain() {
		const { signal } = this.#controller;
		while (!signal.aborted && this.size > 0) {
			while (this.#running.size < this.#concurrency && this.#queue.length > 0) {
				const job = this.#queue.shift();
				this.#running.add(job);
				this.#start(job, signal);
			}
			const done = await new Promise((resolve) => {
				this.#wake = resolve;
				this.once('settled', resolve);
			});
			if (done) yield done;
		}
		this.#idle?.();
	}

	async #start(job, signal) {
		started += 1;
		this.emit('started', job);
		const ok = await job.execute(signal);
		this.#running.delete(job);
		if (ok) {
			finished += 1;
			this.emit('done', job);
			this.emit('settled', job);
			return;
		}
		if (job.state === 'queued') {
			const delay = BACKOFF[Math.min(job.attempts - 1, BACKOFF.length - 1)];
			this.emit('retry', job, delay);
			await sleep(delay, undefined, { signal }).catch(() => {});
			if (!signal.aborted) this.#queue.unshift(job);
			this.emit('settled', null);
			return;
		}
		failed += 1;
		this.emit('failed', job);
		this.emit('settled', job);
	}

	idle() {
		if (this.size === 0) return Promise.resolve();
		return new Promise((resolve) => {
			this.#idle = resolve;
		});
	}

	stop() {
		this.#controller.abort();
		this.#queue.length = 0;
		this.emit('settled', null);
	}
}

export function withTimeout(promise, ms, signal) {
	let timer;
	const expiry = new Promise((_, reject) => {
		timer = setTimeout(() => reject(new Error(`timed out after ${ms}ms`)), ms);
		signal?.addEventListener('abort', () => reject(signal.reason), { once: true });
	});
	return Promise.race([promise, expiry]).finally(() => clearTimeout(timer));
}

export function stats() {
	return { started, finished, failed, pending: started - finished - failed };
}

export function resetStats() {
	started = 0;
	finished = 0;
	failed = 0;
}

export function group(scheduler, name, runs, options) {
	const jobs = runs.map((run, i) => scheduler.add(`${name}#${i}`, run, options));
	return {
		jobs,
		async settled() {
			await scheduler.idle();
			return jobs.map((job) => (job.state === 'done' ? { ok: true, value: job.result } : { ok: false, error: job.error }));
		},
	};
}

export async function runAll(runs, options = {}) {
	const scheduler = new Scheduler(options);
	const results = new Map();
	scheduler.on('done', (job) => results.set(job.name, job.result));
	for (const [name, run] of Object.entries(runs)) scheduler.add(name, run, options);
	for await (const job of scheduler.drain()) {
		if (job.state === 'failed' && options.failFast) {
			scheduler.stop();
			throw job.error;
		}
	}
	return results;
}
