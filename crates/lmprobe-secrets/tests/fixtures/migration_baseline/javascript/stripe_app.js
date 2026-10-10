// Synthetic migration-baseline fixture. Credentials are fake test values.
const key = "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc";
const stripe = require("stripe")(key);

const token = "ghp_FAKEfetchAuthorizationToken1234567890";
fetch("https://api.example.test", {
  headers: { Authorization: `Bearer ${token}` },
});
