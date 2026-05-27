import { Controller, Post, Get, Param, Body } from '@nestjs/common';
import { RetirementService, RetireDto } from './retirement.service';
import { RetirementRecord } from '../shared';

export interface CertificateVerification {
  id: string;
  credit_id: string;
  buyer: string;
  tonnes_retired: string;
  reason: string;
  retired_at: number;
  tx_hash: string;
  verified: boolean;
  ledger_sequence?: number;
}

@Controller('retirement')
export class RetirementController {
  constructor(private readonly retirementService: RetirementService) {}

  /** POST /retirement — retire a credit */
  @Post()
  retire(@Body() dto: RetireDto): Promise<{ retirementId: string }> {
    return this.retirementService.retire(dto);
  }

  /** GET /retirement/:id — fetch a retirement record */
  @Get(':id')
  getRetirement(@Param('id') id: string): Promise<RetirementRecord> {
    return this.retirementService.getRetirement(id);
  }

  /** GET /retirement/account/:address — list retirements for an account */
  @Get('account/:address')
  getByAccount(@Param('address') address: string): Promise<string[]> {
    return this.retirementService.getRetirementsByAccount(address);
  }

  /** GET /certificates/:id/verify — verify retirement certificate authenticity (public) */
  @Get('certificates/:id/verify')
  verifyCertificate(
    @Param('id') certificateId: string,
  ): Promise<CertificateVerification> {
    return this.retirementService.verifyCertificate(certificateId);
  }
}
