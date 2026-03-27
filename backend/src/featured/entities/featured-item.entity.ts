import { Column, CreateDateColumn, Entity, PrimaryGeneratedColumn } from 'typeorm';

export type FeaturedItemType = 'track' | 'playlist' | 'user';

@Entity('featured_items')
export class FeaturedItem {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ type: 'varchar', length: 20 })
  type: FeaturedItemType;

  @Column()
  scUrn: string;

  @Column({ type: 'int', default: 1 })
  weight: number;

  @Column({ default: true })
  active: boolean;

  @CreateDateColumn()
  createdAt: Date;
}
