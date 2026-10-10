// Synthetic source-baseline fixture. Credentials are fake test values.
package main

import (
	"os"

	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/stripe/stripe-go/v85"
)

func main() {
	id := os.Getenv("AWS_ACCESS_KEY_ID")
	secret := os.Getenv("AWS_SECRET_ACCESS_KEY")
	_ = credentials.NewStaticCredentialsProvider(id, secret, "")

	if key, ok := os.LookupEnv("STRIPE_API_KEY"); ok {
		stripe.Key = key
	}

	aws_secret_access_key := "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzabcD12345"
	_ = credentials.NewStaticCredentialsProvider(id, aws_secret_access_key, "")

	api_key := "sk_REMOVEMElive_4eC39HqLyjWDarjtT1zdp7dc"
	stripe.Key = api_key
}
