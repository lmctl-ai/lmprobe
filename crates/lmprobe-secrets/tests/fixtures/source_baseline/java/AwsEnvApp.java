// Synthetic source-baseline fixture. Credentials are fake test values.
import com.amazonaws.auth.BasicAWSCredentials;

class AwsEnvApp {
  void configure() {
    String id = System.getProperty("AWS_ACCESS_KEY_ID");
    String secret = System.getenv("AWS_SECRET_ACCESS_KEY");
    new BasicAWSCredentials(id, secret);

    String literalSecret = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYzabcD12345";
    new BasicAWSCredentials(id, literalSecret);

    String fallback = "ghp_FAKEsourceBaselineJavaToken1234567890";
    client.login(fallback);
  }
}
