import { CacheBudget, managedCache, serializedBytes } from './cacheBudget';
import type { AiMessage, AiTranscript } from './types';

let owner = 0;
/** One visible transcript page; previous pages remain in the provider's file. */
export class TranscriptWindow {
  messages = $state<AiMessage[]>([]);
  hasOlder = $state(false);
  hasNewer = $state(false);
  budgetLimited = $state(false);
  note = $state<string | null>(null);
  private key = `transcript:${++owner}`;

  constructor(private budget: CacheBudget = managedCache) {}

  get before(): number | undefined {
    return this.messages.find((message) => message.id > 0)?.id;
  }
  get after(): number | undefined {
    return this.messages.findLast((message) => message.id > 0)?.id;
  }
  accept(page: AiTranscript): void {
    this.messages = page.messages;
    this.hasOlder = page.hasOlder;
    this.hasNewer = page.hasNewer;
    this.note = page.note;
    this.budgetLimited = false;
    this.account();
  }
  appendLocal(turn: AiMessage[]): void {
    // A locally completed run follows the latest provider turn, even when
    // the user was looking at an old page. Never imply a continuous gap.
    const previous = this.hasNewer ? [] : this.messages;
    const combined = [...previous, ...turn];
    this.hasOlder ||= combined.length > 20 || this.hasNewer;
    this.hasNewer = false;
    this.messages = combined.slice(-20);
    this.account();
  }
  clear(): void {
    this.messages = [];
    this.hasOlder = false;
    this.hasNewer = false;
    this.budgetLimited = false;
    this.note = null;
    this.budget.release(this.key);
  }
  private account(): void {
    this.budget.hold(this.key, this.size(), (maximum) => {
      while (this.size() > maximum && this.messages.length) {
        this.messages = this.messages.slice(1);
        this.hasOlder = true;
        this.budgetLimited = true;
      }
      return this.size();
    });
  }
  private size(): number {
    return this.messages.length ? serializedBytes(this.messages) : 0;
  }
  dispose(): void {
    this.clear();
  }
}
