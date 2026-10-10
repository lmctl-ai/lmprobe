# Synthetic source-baseline fixture. Credentials are fake test values.
import os
import boto3
import stripe

aws_secret = os.environ["AWS_SECRET_ACCESS_KEY"]
boto3.client("s3", aws_secret_access_key=aws_secret)

stripe_key = os.getenv("STRIPE_API_KEY")
stripe.api_key = stripe_key

aws_secret_literal = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzabcD12345"
boto3.client("s3", aws_secret_access_key=aws_secret_literal)

stripe_literal = "sk_REMOVEMElive_51QFakeStripeKeyForSourceBaseline"
stripe.api_key = stripe_literal

fallback_token = "ghp_FAKEsourceBaselinePythonToken123456789"
client.login(fallback_token)
