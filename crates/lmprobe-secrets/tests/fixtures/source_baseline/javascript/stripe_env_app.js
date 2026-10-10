// Synthetic source-baseline fixture. Credentials are fake test values.
const key = process.env.STRIPE_API_KEY;
const stripe = require("stripe")(key);

const token = process.env["GITHUB_TOKEN"];
const octokit = new Octokit({ auth: token });

const api_key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
const stripeLiteral = require("stripe")(api_key);
