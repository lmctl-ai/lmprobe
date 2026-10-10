import express from "express";
const app = express();

export const PORT = 3000;
export let mode = "development";

export function greet(name) {
  return `Hello, ${name}!`;
}

export class UserService {
  constructor(db) {
    this.db = db;
  }
  async findById(id) {
    return this.db.get(id);
  }
}

export default app;

class AdminService extends UserService {
  @authorize
  deleteUser(id) {
    return this.db.delete(id);
  }
}

module.exports.legacy = require("./compat");
