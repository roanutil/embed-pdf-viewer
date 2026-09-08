import { describe, expect, it } from 'vitest';
import { countCrossing, crossings, resetCrossings, subscribeCrossings } from './crossings';

describe('the crossing counter', () => {
  it('counts without any subscriber attached', () => {
    resetCrossings();
    countCrossing();
    countCrossing(2);
    expect(crossings()).toBe(3);
  });

  it('notifies subscribers on count and reset, and stops after unsubscribe', () => {
    resetCrossings();
    let seen = 0;
    const off = subscribeCrossings(() => void seen++);
    countCrossing();
    resetCrossings();
    expect(seen).toBe(2);
    off();
    countCrossing();
    expect(seen).toBe(2);
    expect(crossings()).toBe(1);
  });
});
