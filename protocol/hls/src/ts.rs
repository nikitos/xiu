use {
    super::errors::MediaError,
    aws_sdk_s3::Client as S3Client,
    aws_sdk_s3::primitives::ByteStream,
    aws_sdk_s3::types::ObjectCannedAcl,
    bytes::BytesMut,
    std::{fs, fs::File, io::Write},
};

pub struct Ts {
    live_path: String,
    s3_client: Option<S3Client>,
    s3_bucket: Option<String>,
    s3_prefix: String,
}

impl Ts {
    pub fn new(path: String, s3_client: Option<S3Client>, s3_bucket: Option<String>, s3_prefix: String) -> Self {
        fs::create_dir_all(path.clone()).unwrap();

        Self {
            live_path: path,
            s3_client,
            s3_bucket,
            s3_prefix,
        }
    }
    pub async fn write(&mut self, data: BytesMut, sequence_no: u32) -> Result<(String, String), MediaError> {
        let ts_file_name = format!("{}.ts", sequence_no);
        let ts_file_path = format!("{}/{}", self.live_path, ts_file_name);
        
        // Write to local file

        // Upload to S3 if configured
        if let (Some(client), Some(bucket)) = (&self.s3_client, &self.s3_bucket) {
            let body = ByteStream::from(data.to_vec());
            let key = format!("{}/{}", self.s3_prefix.to_uppercase(), ts_file_name);
            let _result = client
                .put_object()
                .bucket(bucket)
                .key(&key)
                .acl(ObjectCannedAcl::PublicRead)
                .body(body)
                .send()
                .await
                .map_err(|e| MediaError {
                    value: super::errors::MediaErrorValue::IOError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("S3 upload failed: {}", e),
                    )),
                })?;
        } else {
            let mut ts_file_handler = File::create(ts_file_path.clone())?;
            ts_file_handler.write_all(&data[..])?;
        }

        Ok((ts_file_name, ts_file_path))
    }
    pub fn delete(&mut self, ts_file_name: String) {
        fs::remove_file(ts_file_name).unwrap();
    }
}
