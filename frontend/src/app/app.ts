import { Component, inject } from '@angular/core';
import { RouterOutlet, RouterLink } from '@angular/router';
import { ConnectWalletComponent } from './core/components/connect-wallet.component';
import { LocaleSwitcherComponent } from './core/components/locale-switcher.component';
import { ThemeService } from './core/services/theme.service';
import { TranslationService } from './core/services/translation.service';
import { TranslatePipe } from './core/pipes/translate.pipe';

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [RouterOutlet, RouterLink, ConnectWalletComponent, LocaleSwitcherComponent, TranslatePipe],
  templateUrl: './app.html',
  styleUrl: './app.css',
})
export class App {
  readonly theme = inject(ThemeService);
  readonly i18n = inject(TranslationService);

  constructor() {
    this.i18n.init();
  }
}
