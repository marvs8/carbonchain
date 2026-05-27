import { TestBed } from '@angular/core/testing';
import { ThemeService } from './theme.service';

describe('ThemeService', () => {
  let service: ThemeService;

  beforeEach(() => {
    localStorage.clear();
    document.documentElement.classList.remove('dark');
    // Default: no system preference
    spyOn(window, 'matchMedia').and.returnValue({ matches: false } as MediaQueryList);
    TestBed.configureTestingModule({});
    service = TestBed.inject(ThemeService);
  });

  it('should default to light mode when no preference stored', () => {
    expect(service.isDark()).toBeFalse();
    expect(document.documentElement.classList.contains('dark')).toBeFalse();
  });

  it('should toggle to dark mode', () => {
    service.toggle();
    expect(service.isDark()).toBeTrue();
    expect(document.documentElement.classList.contains('dark')).toBeTrue();
    expect(localStorage.getItem('theme')).toBe('dark');
  });

  it('should toggle back to light mode', () => {
    service.toggle();
    service.toggle();
    expect(service.isDark()).toBeFalse();
    expect(document.documentElement.classList.contains('dark')).toBeFalse();
    expect(localStorage.getItem('theme')).toBe('light');
  });

  it('should restore dark preference from localStorage', () => {
    localStorage.setItem('theme', 'dark');
    document.documentElement.classList.remove('dark');
    // Re-create service to trigger loadPreference
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({});
    const fresh = TestBed.inject(ThemeService);
    expect(fresh.isDark()).toBeTrue();
    expect(document.documentElement.classList.contains('dark')).toBeTrue();
  });

  it('icon should be ☀️ in dark mode and 🌙 in light mode', () => {
    expect(service.icon()).toBe('🌙');
    service.toggle();
    expect(service.icon()).toBe('☀️');
  });
});
