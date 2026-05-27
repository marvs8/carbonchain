/* eslint-disable @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-assignment */
import { Injectable, Logger, NotFoundException, BadRequestException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { StellarService } from '../stellar/stellar.service';
import { StellarKeypairService } from '../stellar/stellar-keypair.service';
import { scValToNative, nativeToScVal } from '@stellar/stellar-sdk';
import { CreditMetadata, CreditStatus } from '../shared';
import { CreditEntity } from './credit.entity';
import {
  ICreditRepository,
  CREDIT_REPOSITORY,
  PageResult,
} from './credit.repository';

export class IssueCreditDto {
  issuerPublicKey: string;
  projectId: string;
  vintageYear: number;
  methodology: string;
  geography: string;
  tonnes: string;
  ipfsHash: string;
}

interface ListCreditsFilter {
  methodology?: string;
  geography?: string;
  vintageYear?: number;
  status?: string;
  minTonnes?: string;
  maxTonnes?: string;
  page: number;
  limit: number;
}

@Injectable()
export class CreditsService {
  private readonly logger = new Logger(CreditsService.name);
  private readonly contractId: string;
  private creditsCache: Map<string, CreditMetadata> = new Map();

  constructor(
    private stellarService: StellarService,
    private configService: ConfigService,
    private keypairService: StellarKeypairService,
    @Inject(CREDIT_REPOSITORY) private readonly creditRepo: ICreditRepository,
  ) {
    this.contractId =
      this.configService.get<string>('CREDIT_REGISTRY_CONTRACT_ID') || '';
  }

  async issueCredit(dto: IssueCreditDto): Promise<{ creditId: string }> {
    this.logger.log(`Issuing credit for project ${dto.projectId}`);
    const args = [
      nativeToScVal(dto.issuerPublicKey, { type: 'address' }),
      nativeToScVal(dto.projectId, { type: 'string' }),
      nativeToScVal(dto.vintageYear, { type: 'u32' }),
      nativeToScVal(dto.methodology, { type: 'string' }),
      nativeToScVal(dto.geography, { type: 'string' }),
      nativeToScVal(BigInt(dto.tonnes), { type: 'i128' }),
      nativeToScVal(dto.ipfsHash, { type: 'string' }),
    ];
    const signer = this.keypairService.getAdminKeypair();
    const response = await this.stellarService.invokeContract(
      this.contractId,
      'submit_credit',
      args,
      signer,
    );
    const rv = (response as unknown as Record<string, unknown>).returnValue;
    const creditId = rv
      ? Buffer.from(
          scValToNative(
            rv as Parameters<typeof scValToNative>[0],
          ) as Uint8Array,
        ).toString('hex')
      : 'unknown';

    // Persist to off-chain index
    const entity = new CreditEntity();
    entity.id = creditId;
    entity.projectId = dto.projectId;
    entity.issuer = dto.issuerPublicKey;
    entity.vintageYear = dto.vintageYear;
    entity.methodology = dto.methodology;
    entity.geography = dto.geography;
    entity.tonnes = dto.tonnes;
    entity.ipfsHash = dto.ipfsHash;
    entity.status = CreditStatus.Pending;
    entity.issuedAt = Math.floor(Date.now() / 1000);
    await this.creditRepo.save(entity);

    return { creditId };
  }

  async getCredit(creditId: string): Promise<CreditMetadata> {
    // Try off-chain index first
    const cached = await this.creditRepo.findById(creditId);
    if (cached) return this.entityToMetadata(cached);

    // Fall back to on-chain read
    try {
      this.logger.log(`Fetching credit metadata for ID: ${creditId}`);
      const args = [
        nativeToScVal(Buffer.from(creditId, 'hex'), { type: 'bytes' }),
      ];
      const retval = await this.stellarService.readContract(
        this.contractId,
        'get_credit',
        args,
      );
      if (!retval)
        throw new NotFoundException(
          `Credit with ID ${creditId} not found on-chain`,
        );
      const native = scValToNative(retval);
      return this.mapToCreditMetadata(creditId, native);
    } catch (error: unknown) {
      this.logger.error(
        `Failed to fetch credit ${creditId}: ${(error as Error).message}`,
      );
      throw error;
    }
  }

  async getBulkCredits(creditIds: string[]): Promise<CreditMetadata[]> {
    if (!creditIds || creditIds.length === 0) {
      throw new BadRequestException('Credit IDs array cannot be empty');
    }
    if (creditIds.length > 100) {
      throw new BadRequestException('Maximum 100 credits per bulk request');
    }

    this.logger.log(`Fetching ${creditIds.length} credits in bulk`);
    const results: CreditMetadata[] = [];

    for (const creditId of creditIds) {
      try {
        const credit = await this.getCredit(creditId);
        results.push(credit);
      } catch (error: unknown) {
        this.logger.warn(
          `Failed to fetch credit ${creditId}: ${(error as Error).message}`,
        );
      }
    }

    return results;
  }

  async listCredits(
    filter: ListCreditsFilter,
  ): Promise<{ data: CreditMetadata[]; total: number; page: number; limit: number }> {
    this.logger.log(`Listing credits with filters: ${JSON.stringify(filter)}`);

    // For now, return empty results as we don't have a list_all_credits contract method
    // In production, this would query the blockchain or an indexed database
    const allCredits: CreditMetadata[] = [];

    // Apply filters
    let filtered = allCredits;

    if (filter.methodology) {
      filtered = filtered.filter(
        (c) => c.methodology.toLowerCase() === filter.methodology?.toLowerCase(),
      );
    }

    if (filter.geography) {
      filtered = filtered.filter(
        (c) => c.geography.toLowerCase() === filter.geography?.toLowerCase(),
      );
    }

    if (filter.vintageYear) {
      filtered = filtered.filter((c) => c.vintage_year === filter.vintageYear);
    }

    if (filter.status) {
      filtered = filtered.filter(
        (c) => c.status.toLowerCase() === filter.status?.toLowerCase(),
      );
    }

    if (filter.minTonnes) {
      const minVal = BigInt(filter.minTonnes);
      filtered = filtered.filter((c) => BigInt(c.tonnes) >= minVal);
    }

    if (filter.maxTonnes) {
      const maxVal = BigInt(filter.maxTonnes);
      filtered = filtered.filter((c) => BigInt(c.tonnes) <= maxVal);
    }

    const total = filtered.length;
    const start = (filter.page - 1) * filter.limit;
    const end = start + filter.limit;
    const data = filtered.slice(start, end);

    return { data, total, page: filter.page, limit: filter.limit };
  }

  async listCreditsByProject(projectId: string): Promise<string[]> {
    try {
      this.logger.log(`Listing credits for project: ${projectId}`);
      const args = [nativeToScVal(projectId, { type: 'string' })];
      const retval = await this.stellarService.readContract(
        this.contractId,
        'list_credits_by_project',
        args,
      );
      if (!retval) return [];
      const native = scValToNative(retval) as Buffer[];
      return native.map((buf) => buf.toString('hex'));
    } catch (error: unknown) {
      this.logger.error(
        `Failed to list credits for project ${projectId}: ${(error as Error).message}`,
      );
      return [];
    }
  }

  private mapToCreditMetadata(id: string, native: any): CreditMetadata {
    return {
      id,
      project_id: String(native.project_id),
      issuer: String(native.issuer),
      vintage_year: Number(native.vintage_year),
      methodology: String(native.methodology),
      geography: String(native.geography),
      tonnes: String(native.tonnes),
      ipfs_hash: String(native.ipfs_hash),
      status: native.status as CreditStatus,
      issued_at: Number(native.issued_at),
    };
  }
}
