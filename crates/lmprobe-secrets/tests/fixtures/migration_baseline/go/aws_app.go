// Synthetic migration-baseline fixture. Credentials are fake test values.
package main

import (
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/credentials"
)

func main() {
	id := "AKIAIOSFODNN7EXAMPLE"
	secret := "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzabcD12345"
	_ = credentials.NewStaticCredentialsProvider(id, secret, "")
	_ = aws.Credentials{AccessKeyID: id, SecretAccessKey: secret}
}
