import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { ScheduleModule } from '@nestjs/schedule';
import { AppController } from './app.controller';
import { AppService } from './app.service';
import { StellarModule } from './stellar/stellar.module';
import { CreditsModule } from './credits/credits.module';
import { ProjectsModule } from './projects/projects.module';
import { AuthModule } from './auth/auth.module';
import { VerifiersModule } from './verifiers/verifiers.module';
import { RetirementModule } from './retirement/retirement.module';
import { MarketplaceModule } from './marketplace/marketplace.module';
import { EventsModule } from './events/events.module';
import { WebhooksModule } from './webhooks/webhooks.module';

@Module({
  imports: [
    ConfigModule.forRoot({
      isGlobal: true,
    }),
    ScheduleModule.forRoot(),
    StellarModule,
    CreditsModule,
    ProjectsModule,
    AuthModule,
    VerifiersModule,
    RetirementModule,
    MarketplaceModule,
    EventsModule,
    WebhooksModule,
  ],
  controllers: [AppController],
  providers: [AppService],
})
export class AppModule {}
