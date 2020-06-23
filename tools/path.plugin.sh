# A sourceable plugin script for adding the `fs_image/tools` directory to your path
# so that one can simply use `buck` and let the shell do it's thing.

tools_dir="$(dirname $(realpath ${0}))"

insert_tools_in_path(){
  for dir in $(echo -e "${PATH//:/"\n"}"); do
    if [[ "${dir}" == "${tools_dir}" ]]; then
      echo ${PATH}
      return
    fi
  done

  echo ${tools_dir}:${PATH}
}

PATH=$(insert_tools_in_path)
