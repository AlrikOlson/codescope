import { formatName, capitalize } from './utils';

export interface AppConfig {
  title: string;
  debug: boolean;
}

export class App {
  private config: AppConfig;

  constructor(config: AppConfig) {
    this.config = config;
  }

  getName(): string {
    return formatName(this.config.title);
  }

  isDebug(): boolean {
    return this.config.debug;
  }

  run(): void {
    const name = this.getName();
    console.log(`Running ${capitalize(name)}`);
  }
}
