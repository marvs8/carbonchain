import { Injectable, signal, computed } from '@angular/core';

export type Locale = 'en' | 'es' | 'fr';

const STORAGE_KEY = 'locale';

@Injectable({ providedIn: 'root' })
export class TranslationService {
  private translations: Record<string, string> = {};
  private readonly _locale = signal<Locale>(
    (localStorage.getItem(STORAGE_KEY) as Locale) ?? 'en',
  );

  readonly locale = this._locale.asReadonly();
  readonly locales: { code: Locale; label: string }[] = [
    { code: 'en', label: 'EN' },
    { code: 'es', label: 'ES' },
    { code: 'fr', label: 'FR' },
  ];

  async setLocale(locale: Locale): Promise<void> {
    const res = await fetch(`/assets/i18n/${locale}.json`);
    this.translations = await res.json();
    this._locale.set(locale);
    localStorage.setItem(STORAGE_KEY, locale);
  }

  t(key: string): string {
    return this.translations[key] ?? key;
  }

  async init(): Promise<void> {
    await this.setLocale(this._locale());
  }
}
