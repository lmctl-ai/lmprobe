# Synthetic migration-baseline fixture. Credentials are fake test values.
import boto3
import stripe

aws_secret = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzabcD12345"
boto3.client("s3", aws_secret_access_key=aws_secret)

stripe_key = "sk_REMOVEMElive_51QFakeStripeKeyForLmprobeMigration"
stripe.api_key = stripe_key

token = "ghp_FAKEtokenForMigrationBaseline1234567890"
client.login(token)
