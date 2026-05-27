/* eslint-disable @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-assignment */
import { Injectable, Logger, NotFoundException, Inject } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { StellarService } from '../stellar/stellar.service';
import { StellarKeypairService } from '../stellar/stellar-keypair.service';
import { nativeToScVal, scValToNative } from '@stellar/stellar-sdk';
import { RetirementRecord } from '../shared';
import { RetirementEntity } from './retirement.entity';
import {
  IRetirementRepository,
  RETIREMENT_REPOSITORY,
} from './retirement.repository';
import { PageResult } from '../credits/credit.repository';

export class RetireDto {
  buyerPublicKey: string;
  creditId: string;
  tonnes: string;
  reason: string;
}

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

@Injectable()
export class RetirementService {
  private readonly logger = new Logger(RetirementService.name);
  private readonly retirementContractId: string;
  private readonly registryContractId: string;

  constructor(
    private readonly stellarService: StellarService,
    private readonly keypairService: StellarKeypairService,
    private readonly configService: ConfigService,
    @Inject(RETIREMENT_REPOSITORY) private readonly retirementRepo: IRetirementRepository,
  ) {
    this.retirementContractId = this.configService.get<string>(
      'RETIREMENT_CONTRACT_ID',
      '',
    );
    this.registryContractId = this.configService.get<string>(
      'CREDIT_REGISTRY_CONTRACT_ID',
      '',
    );
  }

  async retire(dto: RetireDto): Promise<{ retirementId: string }> {
    this.logger.log(
      `Retiring credit ${dto.creditId} for ${dto.buyerPublicKey}`,
    );
    const args = [
      nativeToScVal(dto.buyerPublicKey, { type: 'address' }),
      nativeToScVal(Buffer.from(dto.creditId, 'hex'), { type: 'bytes' }),
      nativeToScVal(BigInt(dto.tonnes), { type: 'i128' }),
      nativeToScVal(dto.reason, { type: 'string' }),
      nativeToScVal(this.registryContractId, { type: 'address' }),
    ];
    const signer = this.keypairService.getAdminKeypair();
    const response = await this.stellarService.invokeContract(
      this.retirementContractId,
      'retire',
      args,
      signer,
    );
    const rv = (response as unknown as Record<string, unknown>).returnValue;
    const retirementId = rv
      ? Buffer.from(
          scValToNative(
            rv as Parameters<typeof scValToNative>[0],
          ) as Uint8Array,
        ).toString('hex')
      : 'unknown';

    // Persist to off-chain index
    const entity = new RetirementEntity();
    entity.id = retirementId;
    entity.creditId = dto.creditId;
    entity.buyer = dto.buyerPublicKey;
    entity.tonnesRetired = dto.tonnes;
    entity.reason = dto.reason;
    entity.retiredAt = Math.floor(Date.now() / 1000);
    entity.txHash = '';
    await this.retirementRepo.save(entity);

    return { retirementId };
  }

  async getRetirement(retirementId: string): Promise<RetirementRecord> {
    // Try off-chain index first
    const cached = await this.retirementRepo.findById(retirementId);
    if (cached) return this.entityToRecord(cached);

    // Fall back to on-chain read
    const args = [
      nativeToScVal(Buffer.from(retirementId, 'hex'), { type: 'bytes' }),
    ];
    const retval = await this.stellarService.readContract(
      this.retirementContractId,
      'get_retirement',
      args,
    );
    if (!retval)
      throw new NotFoundException(`Retirement ${retirementId} not found`);

    const n = scValToNative(retval);
    return {
      id: retirementId,
      credit_id: Buffer.from(n.credit_id as Uint8Array).toString('hex'),
      buyer: String(n.buyer),
      tonnes_retired: String(n.tonnes_retired),
      reason: String(n.reason),
      retired_at: Number(n.retired_at),
      tx_hash: '',
    };
  }

  async listRetirements(page = 1, limit = 20): Promise<PageResult<RetirementRecord>> {
    const result = await this.retirementRepo.findAll(page, limit);
    return { ...result, data: result.data.map((e) => this.entityToRecord(e)) };
  }

  async getRetirementsByAccount(account: string, page = 1, limit = 20): Promise<PageResult<RetirementRecord>> {
    const result = await this.retirementRepo.findByBuyer(account, page, limit);
    return { ...result, data: result.data.map((e) => this.entityToRecord(e)) };
  }

  private entityToRecord(e: RetirementEntity): RetirementRecord {
    return {
      id: e.id,
      credit_id: e.creditId,
      buyer: e.buyer,
      tonnes_retired: e.tonnesRetired,
      reason: e.reason,
      retired_at: e.retiredAt,
      tx_hash: e.txHash,
    };
  }

  async verifyCertificate(
    certificateId: string,
  ): Promise<CertificateVerification> {
    try {
      this.logger.log(`Verifying certificate: ${certificateId}`);
      const retirement = await this.getRetirement(certificateId);

      // Fetch transaction details from Stellar to verify on-chain proof
      const txHash = await this.stellarService.getTransactionHash(
        certificateId,
      );
      const ledgerSequence = await this.stellarService.getLedgerSequence(
        certificateId,
      );

      return {
        id: retirement.id,
        credit_id: retirement.credit_id,
        buyer: retirement.buyer,
        tonnes_retired: retirement.tonnes_retired,
        reason: retirement.reason,
        retired_at: retirement.retired_at,
        tx_hash: txHash || '',
        verified: true,
        ledger_sequence: ledgerSequence,
      };
    } catch (error: unknown) {
      this.logger.error(
        `Failed to verify certificate ${certificateId}: ${(error as Error).message}`,
      );
      throw new NotFoundException(
        `Certificate ${certificateId} not found or cannot be verified`,
      );
    }
  }
}
