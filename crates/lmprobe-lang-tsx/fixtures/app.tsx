import React from "react";

export const PORT = 3000;
export let mode = "development";

export function greet(name: string): string {
  return `Hello, ${name}!`;
}

export class UserService {
  constructor(private db: any) {}
  async findById(id: string) {
    return this.db.get(id);
  }
}

export default function App() {
  return <div>Hello</div>;
}

class AdminService extends UserService {
  @authorize
  deleteUser(id: string) {
    return this.db.delete(id);
  }
}

module.exports.legacy = require("./compat");
