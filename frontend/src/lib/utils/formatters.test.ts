import { describe, it, expect } from 'vitest';
import {
	formatCorrelation,
	correlationColor,
	formatNumber,
	formatPrice,
	formatPValue,
	changeColor,
	changeArrow
} from './formatters';

describe('formatCorrelation', () => {
	it('formats positive values to 4 decimal places', () => {
		expect(formatCorrelation(0.12345)).toBe('0.1235');
	});

	it('formats negative values', () => {
		expect(formatCorrelation(-0.5678)).toBe('-0.5678');
	});

	it('formats zero', () => {
		expect(formatCorrelation(0)).toBe('0.0000');
	});

	it('formats edge values', () => {
		expect(formatCorrelation(1)).toBe('1.0000');
		expect(formatCorrelation(-1)).toBe('-1.0000');
	});
});

describe('correlationColor', () => {
	it('returns blue for positive values', () => {
		expect(correlationColor(0.5)).toContain('accent-blue');
	});

	it('returns red for negative values', () => {
		expect(correlationColor(-0.5)).toContain('accent-red');
	});

	it('returns secondary for zero', () => {
		expect(correlationColor(0)).toContain('text-secondary');
	});
});

describe('formatNumber', () => {
	it('formats billions', () => {
		expect(formatNumber(1_500_000_000)).toBe('1.5B');
	});

	it('formats millions', () => {
		expect(formatNumber(2_300_000)).toBe('2.3M');
	});

	it('formats thousands', () => {
		expect(formatNumber(4_500)).toBe('4.5K');
	});

	it('formats small numbers with locale string', () => {
		expect(formatNumber(42)).toBe('42');
	});

	it('formats zero', () => {
		expect(formatNumber(0)).toBe('0');
	});
});

describe('formatPrice', () => {
	it('formats with 2 decimal places', () => {
		const result = formatPrice(1234.5);
		expect(result).toContain('1');
		expect(result).toContain('234');
		expect(result).toContain('.50');
	});

	it('formats large numbers', () => {
		const result = formatPrice(1000000);
		expect(result).toContain('.00');
	});
});

describe('formatPValue', () => {
	it('returns -- for null', () => {
		expect(formatPValue(null)).toBe('--');
	});

	it('returns < 0.001 for very small values', () => {
		expect(formatPValue(0.0001)).toBe('< 0.001');
	});

	it('formats moderate values to 4 decimal places', () => {
		expect(formatPValue(0.0456)).toBe('0.0456');
	});

	it('handles edge case at boundary', () => {
		expect(formatPValue(0.001)).toBe('0.0010');
	});
});

describe('changeColor', () => {
	it('returns green for positive', () => {
		expect(changeColor(5)).toContain('accent-green');
	});

	it('returns red for negative', () => {
		expect(changeColor(-3)).toContain('accent-red');
	});

	it('returns secondary for zero', () => {
		expect(changeColor(0)).toContain('text-secondary');
	});
});

describe('changeArrow', () => {
	it('returns + for positive', () => {
		expect(changeArrow(5)).toBe('+');
	});

	it('returns empty for negative', () => {
		expect(changeArrow(-3)).toBe('');
	});

	it('returns empty for zero', () => {
		expect(changeArrow(0)).toBe('');
	});
});
