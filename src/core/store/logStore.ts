export class LogStore {
  private logs: string[] = [];

  add(log: string) {
    this.logs.push(log);
  }

  getAll() {
    return this.logs;
  }
}