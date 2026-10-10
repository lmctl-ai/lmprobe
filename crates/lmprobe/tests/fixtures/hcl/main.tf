module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "5.0.0"

  name = "demo-vpc"
  cidr = "10.0.0.0/16"
}

resource "aws_s3_bucket" "logs" {
  bucket = "demo-logs"
}
