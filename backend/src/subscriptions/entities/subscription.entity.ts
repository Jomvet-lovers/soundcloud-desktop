import { Column, Entity, PrimaryColumn } from 'typeorm';

@Entity('subscriptions')
export class Subscription {
  @PrimaryColumn()
  userUrn: string;

  @Column({ type: 'bigint' })
  expDate: number;
}
